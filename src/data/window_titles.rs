//! Tmux window title management.
//!
//! Prefixes agent windows with an activity icon (🔔 💬 ⏳ 🤖 ✅) and restores
//! the original titles — including the window's automatic-rename setting,
//! which tmux disables whenever a window is renamed manually.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::warn;

use crate::data::sync::GatheredData;
use crate::data::tmux;
use crate::model::session::AgentActivity;

/// All icon prefixes we may prepend to window names.
const ACTIVITY_ICONS: &[&str] = &["🔔", "💬", "⏳", "🤖", "✅"];

/// How long a window keeps its ✅ icon after the agent finishes.
const COMPLETED_EXPIRY: Duration = Duration::from_secs(600);

/// Pick an icon prefix for the given activity.
fn activity_icon(activity: &AgentActivity) -> &'static str {
    match activity {
        AgentActivity::WaitingForPermission => "🔔",
        AgentActivity::WaitingForInput => "💬",
        AgentActivity::Processing => "⏳",
        AgentActivity::Completed => "✅",
        AgentActivity::Unknown => "🤖",
    }
}

/// Strip any known activity icon prefix from a window name, returning the base name.
fn strip_icon_prefix(name: &str) -> &str {
    for icon in ACTIVITY_ICONS {
        if let Some(rest) = name.strip_prefix(icon) {
            return rest.trim_start();
        }
    }
    name
}

/// Per-window activity, highest-priority pane wins. Keyed by stable
/// window id ("@N").
fn aggregate_window_activities(data: &GatheredData) -> HashMap<String, AgentActivity> {
    let mut window_activities: HashMap<String, AgentActivity> = HashMap::new();
    for pane in &data.agent_panes {
        let activity = data.activity_for(pane);
        window_activities
            .entry(pane.window_id.clone())
            .and_modify(|entry| {
                if activity.sort_priority() < entry.sort_priority() {
                    *entry = activity.clone();
                }
            })
            .or_insert(activity);
    }
    window_activities
}

/// Drives icon prefixes on tmux window names from agent activity.
///
/// Owns all cross-cycle state: previous activities (for completion-transition
/// detection), windows in the ✅ grace period, which windows we've renamed,
/// and each window's original automatic-rename setting.
pub struct TitleManager {
    prev_activities: HashMap<String, AgentActivity>,
    completed: HashMap<String, Instant>,
    renamed: HashSet<String>,
    auto_rename_was_on: HashMap<String, bool>,
}

impl TitleManager {
    pub fn new() -> Self {
        Self {
            prev_activities: HashMap::new(),
            completed: HashMap::new(),
            renamed: HashSet::new(),
            auto_rename_was_on: HashMap::new(),
        }
    }

    /// Compute per-window activity, detect completion transitions, and apply
    /// icon renames. Called once per sync cycle.
    pub async fn apply(&mut self, data: &GatheredData) -> Result<()> {
        let mut window_activities = aggregate_window_activities(data);
        let now = Instant::now();

        // Completion transitions: the agent finished work (Processing →
        // WaitingForInput) or its pane disappeared entirely (exited).
        for (window_id, prev) in &self.prev_activities {
            let just_completed = matches!(
                (prev, window_activities.get(window_id)),
                (AgentActivity::Processing, Some(AgentActivity::WaitingForInput)) | (_, None)
            );
            if just_completed && !self.completed.contains_key(window_id) {
                self.completed.insert(window_id.clone(), now);
            }
        }

        // If the agent starts processing again, clear its completed state.
        for (window_id, activity) in &window_activities {
            if matches!(activity, AgentActivity::Processing) {
                self.completed.remove(window_id);
            }
        }

        // Expire ✅ windows past the grace period; restore titles for windows
        // whose agent exited (no live agent pane anymore).
        let expired: Vec<String> = self
            .completed
            .iter()
            .filter(|(_, ts)| now.duration_since(**ts) >= COMPLETED_EXPIRY)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired {
            self.completed.remove(id);
        }
        let exited: Vec<String> = expired
            .into_iter()
            .filter(|id| !window_activities.contains_key(id))
            .collect();
        if !exited.is_empty() {
            if let Err(e) = self.restore(&exited).await {
                warn!(error = %e, "failed to restore expired completed windows");
            }
        }

        // Windows in the grace period show ✅ regardless of pane state.
        for window_id in self.completed.keys() {
            window_activities.insert(window_id.clone(), AgentActivity::Completed);
        }

        // Save for next cycle's transition detection, excluding the ✅
        // override so a later real activity change is still detected.
        self.prev_activities = window_activities
            .iter()
            .filter(|(_, a)| !matches!(a, AgentActivity::Completed))
            .map(|(id, a)| (id.clone(), a.clone()))
            .collect();

        self.rename(&window_activities).await
    }

    /// Apply icon renames for the given window activities in one tmux call.
    async fn rename(
        &mut self,
        window_activities: &HashMap<String, AgentActivity>,
    ) -> Result<()> {
        if window_activities.is_empty() {
            return Ok(());
        }

        let windows = tmux::get_all_windows().await?;
        let mut commands: Vec<Vec<String>> = Vec::new();

        for (window_id, activity) in window_activities {
            let info = match windows.get(window_id) {
                Some(i) => i,
                None => continue,
            };

            // Record the original automatic-rename setting the first time we
            // touch a window — our rename below will force it off.
            if self.renamed.insert(window_id.clone()) {
                self.auto_rename_was_on
                    .insert(window_id.clone(), info.automatic_rename);
            }

            let base_name = strip_icon_prefix(&info.name);
            let new_name = format!("{} {}", activity_icon(activity), base_name);
            if new_name != info.name {
                commands.push(vec![
                    "rename-window".to_string(),
                    "-t".to_string(),
                    window_id.clone(),
                    new_name,
                ]);
            }
        }

        tmux::run_chained(&commands).await
    }

    /// Strip icons from the given windows and restore their original
    /// automatic-rename setting, forgetting their tracked state.
    async fn restore(&mut self, window_ids: &[String]) -> Result<()> {
        if window_ids.is_empty() {
            return Ok(());
        }

        let windows = tmux::get_all_windows().await?;
        let mut commands: Vec<Vec<String>> = Vec::new();

        for window_id in window_ids {
            if let Some(info) = windows.get(window_id) {
                let base_name = strip_icon_prefix(&info.name);
                if base_name != info.name {
                    commands.push(vec![
                        "rename-window".to_string(),
                        "-t".to_string(),
                        window_id.clone(),
                        base_name.to_string(),
                    ]);
                }
                if self.auto_rename_was_on.get(window_id).copied().unwrap_or(false) {
                    commands.push(vec![
                        "set-option".to_string(),
                        "-w".to_string(),
                        "-t".to_string(),
                        window_id.clone(),
                        "automatic-rename".to_string(),
                        "on".to_string(),
                    ]);
                }
            }
            self.renamed.remove(window_id);
            self.auto_rename_was_on.remove(window_id);
        }

        tmux::run_chained(&commands).await
    }

    /// Restore every window we've touched. Called on shutdown.
    pub async fn reset_all(&mut self) -> Result<()> {
        let all: Vec<String> = self.renamed.iter().cloned().collect();
        self.restore(&all).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::stats::ProcessTree;
    use crate::data::tmux::AgentPaneInfo;
    use crate::model::session::AgentType;
    use crate::model::tmux::TmuxPaneRef;

    #[test]
    fn strip_icon_prefix_removes_bell() {
        assert_eq!(strip_icon_prefix("🔔 claude"), "claude");
    }

    #[test]
    fn strip_icon_prefix_removes_speech() {
        assert_eq!(strip_icon_prefix("💬 my-window"), "my-window");
    }

    #[test]
    fn strip_icon_prefix_removes_hourglass() {
        assert_eq!(strip_icon_prefix("⏳ zsh"), "zsh");
    }

    #[test]
    fn strip_icon_prefix_leaves_plain_name() {
        assert_eq!(strip_icon_prefix("claude"), "claude");
    }

    #[test]
    fn strip_icon_prefix_removes_robot() {
        assert_eq!(strip_icon_prefix("🤖 claude"), "claude");
    }

    #[test]
    fn strip_icon_prefix_removes_checkmark() {
        assert_eq!(strip_icon_prefix("✅ claude"), "claude");
    }

    #[test]
    fn strip_icon_prefix_leaves_other_emoji() {
        assert_eq!(strip_icon_prefix("✳ claude"), "✳ claude");
    }

    fn pane_in_window(window_id: &str, pane_id: &str, activity: AgentActivity) -> AgentPaneInfo {
        AgentPaneInfo {
            tmux_ref: TmuxPaneRef {
                session_name: "main".to_string(),
                window_index: 0,
                pane_id: pane_id.to_string(),
            },
            window_id: window_id.to_string(),
            pid: 1,
            title: String::new(),
            current_path: "/tmp".to_string(),
            agent_type: AgentType::Claude,
            title_activity: activity,
            pane_active: false,
        }
    }

    fn gathered(panes: Vec<AgentPaneInfo>) -> GatheredData {
        GatheredData {
            agent_panes: panes,
            session_files: Vec::new(),
            project_sessions: Vec::new(),
            process_tree: ProcessTree::empty(),
            pane_activities: HashMap::new(),
        }
    }

    #[test]
    fn aggregate_single_idle_pane_keeps_its_activity() {
        let data = gathered(vec![pane_in_window(
            "@1",
            "%1",
            AgentActivity::WaitingForInput,
        )]);
        let activities = aggregate_window_activities(&data);
        assert_eq!(
            activities.get("@1"),
            Some(&AgentActivity::WaitingForInput)
        );
    }

    #[test]
    fn aggregate_highest_priority_pane_wins() {
        let data = gathered(vec![
            pane_in_window("@1", "%1", AgentActivity::WaitingForInput),
            pane_in_window("@1", "%2", AgentActivity::WaitingForPermission),
            pane_in_window("@1", "%3", AgentActivity::Processing),
        ]);
        let activities = aggregate_window_activities(&data);
        assert_eq!(
            activities.get("@1"),
            Some(&AgentActivity::WaitingForPermission)
        );
    }

    #[test]
    fn aggregate_refined_activity_overrides_title_activity() {
        let mut data = gathered(vec![pane_in_window(
            "@1",
            "%1",
            AgentActivity::WaitingForInput,
        )]);
        data.pane_activities
            .insert("%1".to_string(), AgentActivity::WaitingForPermission);
        let activities = aggregate_window_activities(&data);
        assert_eq!(
            activities.get("@1"),
            Some(&AgentActivity::WaitingForPermission)
        );
    }
}

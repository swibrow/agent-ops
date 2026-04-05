use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use tracing::warn;

use crate::agent::{AgentProvider, AgentSessionFile};
use crate::config::Config;
use crate::data::tmux;
use crate::model::session::{AgentActivity, AgentType};
use crate::model::tmux::TmuxPane;

pub struct ClaudeProvider;

impl AgentProvider for ClaudeProvider {
    fn agent_type(&self) -> AgentType {
        AgentType::Claude
    }

    fn detect_pane(&self, pane: &TmuxPane) -> bool {
        // Title-based detection (most specific)
        pane.title.starts_with('\u{2733}') // ✳ sparkle (idle)
            || has_braille_prefix(&pane.title) // braille spinner (processing)
            || pane.current_command == "claude"
    }

    fn detect_activity(&self, pane: &TmuxPane) -> AgentActivity {
        if has_braille_prefix(&pane.title) {
            AgentActivity::Processing
        } else if pane.title.starts_with('\u{2733}') {
            AgentActivity::WaitingForInput
        } else {
            AgentActivity::Unknown
        }
    }

    fn clean_title<'a>(&self, title: &'a str) -> &'a str {
        if let Some(first) = title.chars().next() {
            if ('\u{2800}'..='\u{28FF}').contains(&first) || first == '\u{2733}' {
                return title[first.len_utf8()..].trim();
            }
        }
        title
    }

    fn gather_sessions(
        &self,
        config: &Config,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<AgentSessionFile>>> + Send + '_>> {
        let claude_dirs = config.claude_dirs.clone();
        Box::pin(async move {
            let mut result = Vec::new();
            for claude_dir in &claude_dirs {
                let sessions_dir = claude_dir.join("sessions");
                if !sessions_dir.exists() {
                    continue;
                }

                for entry in std::fs::read_dir(&sessions_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            if let Ok(session) =
                                serde_json::from_str::<crate::data::claude::ClaudeSessionFile>(
                                    &content,
                                )
                            {
                                result.push(AgentSessionFile {
                                    session_id: session.session_id,
                                    pid: session.pid,
                                    cwd: session.cwd,
                                    started_at: session.started_at,
                                });
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
            Ok(result)
        })
    }

    fn refine_activity(
        &self,
        pane_id: &str,
        base: AgentActivity,
    ) -> Pin<Box<dyn Future<Output = AgentActivity> + Send + '_>> {
        let pane_id = pane_id.to_string();
        Box::pin(async move {
            if base == AgentActivity::Processing {
                return AgentActivity::Processing;
            }

            let content = match tmux::capture_pane(&pane_id, 15).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(pane_id, error = %e, "failed to capture pane content");
                    return base;
                }
            };

            let last_lines: Vec<&str> = content.lines().rev().take(30).collect();
            let has_permission = last_lines.iter().any(|line| {
                let trimmed = line.trim();
                // Explicit permission prompts
                trimmed.contains("Do you want to proceed?")
                    || trimmed.contains("Yes, and don't ask again")
                    || trimmed.contains("Esc to cancel")
                    || trimmed.contains("approve this action")
                    // Selection menu options (numbered Yes/No choices)
                    || trimmed == "1. Yes"
                    || trimmed == "2. No"
                    || trimmed.contains("1. Yes")
                        && trimmed.contains('\u{276F}') // ❯ selection indicator
                    // Tool approval headers
                    || trimmed.starts_with("Bash command")
                    || trimmed.starts_with("Edit file")
                    || trimmed.starts_with("Write file")
                    || trimmed.starts_with("Create file")
                    || trimmed.starts_with("Run command")
                    // Safety warnings that precede permission prompts
                    || trimmed.contains("Command contains")
                    || trimmed.contains("Shell operators detected")
            });

            if has_permission {
                AgentActivity::WaitingForPermission
            } else {
                base
            }
        })
    }
}

fn has_braille_prefix(title: &str) -> bool {
    title
        .chars()
        .next()
        .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pane(command: &str, title: &str) -> TmuxPane {
        TmuxPane {
            id: "%1".to_string(),
            pid: 1234,
            title: title.to_string(),
            current_command: command.to_string(),
            current_path: "/tmp".to_string(),
            active: false,
        }
    }

    #[test]
    fn detect_sparkle_title() {
        let p = ClaudeProvider;
        assert!(p.detect_pane(&make_pane("node", "\u{2733} Claude Code")));
    }

    #[test]
    fn detect_braille_title() {
        let p = ClaudeProvider;
        assert!(p.detect_pane(&make_pane("node", "\u{2801} Processing")));
    }

    #[test]
    fn detect_claude_command() {
        let p = ClaudeProvider;
        assert!(p.detect_pane(&make_pane("claude", "plain title")));
    }

    #[test]
    fn no_detect_plain_pane() {
        let p = ClaudeProvider;
        assert!(!p.detect_pane(&make_pane("vim", "editing")));
    }

    #[test]
    fn activity_processing() {
        let p = ClaudeProvider;
        assert_eq!(
            p.detect_activity(&make_pane("node", "\u{2801} working")),
            AgentActivity::Processing
        );
    }

    #[test]
    fn activity_waiting() {
        let p = ClaudeProvider;
        assert_eq!(
            p.detect_activity(&make_pane("node", "\u{2733} idle")),
            AgentActivity::WaitingForInput
        );
    }

    #[test]
    fn clean_title_strips_sparkle() {
        let p = ClaudeProvider;
        assert_eq!(p.clean_title("\u{2733} Claude Code"), "Claude Code");
    }

    #[test]
    fn clean_title_strips_braille() {
        let p = ClaudeProvider;
        assert_eq!(p.clean_title("\u{2801} Processing"), "Processing");
    }

    #[test]
    fn clean_title_plain() {
        let p = ClaudeProvider;
        assert_eq!(p.clean_title("plain title"), "plain title");
    }
}

use anyhow::Result;
use tokio::process::Command;
use tracing::warn;

use crate::model::tmux::{TmuxPane, TmuxPaneRef};

const SEP: &str = "|||";

/// List all panes across all tmux sessions in a single call.
/// Returns a flat list of (session_name, window_index, pane).
pub async fn list_all_panes() -> Result<Vec<(String, u32, TmuxPane)>> {
    let format_str = format!(
        "#{{session_name}}{SEP}#{{window_index}}{SEP}#{{pane_id}}{SEP}#{{pane_pid}}{SEP}#{{pane_title}}{SEP}#{{pane_current_command}}{SEP}#{{pane_current_path}}"
    );

    let output = Command::new("tmux")
        .args(["list-panes", "-a", "-F", &format_str])
        .output()
        .await?;

    if !output.status.success() {
        // tmux not running — not an error
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut panes = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(SEP).collect();
        if parts.len() < 7 {
            continue;
        }

        let session_name = parts[0].to_string();
        let window_index: u32 = parts[1].parse().unwrap_or(0);

        let pane = TmuxPane {
            id: parts[2].to_string(),
            pid: parts[3].parse().unwrap_or(0),
            title: parts[4].to_string(),
            current_command: parts[5].to_string(),
            current_path: parts[6].to_string(),
        };

        panes.push((session_name, window_index, pane));
    }

    Ok(panes)
}

/// Collect all Claude agent panes with their tmux refs.
pub async fn find_agent_panes() -> Result<Vec<AgentPaneInfo>> {
    let all_panes = list_all_panes().await?;
    let mut agents = Vec::new();

    for (session_name, window_index, pane) in all_panes {
        if pane.is_claude_agent() {
            let title_activity = pane.detect_activity();
            agents.push(AgentPaneInfo {
                tmux_ref: TmuxPaneRef {
                    session_name,
                    window_index,
                    pane_id: pane.id.clone(),
                },
                pid: pane.pid,
                title: pane.clean_title().to_string(),
                current_path: pane.current_path.clone(),
                title_activity,
            });
        }
    }

    Ok(agents)
}

pub async fn capture_pane(pane_id: &str, lines: u32) -> Result<String> {
    let output = Command::new("tmux")
        .args([
            "capture-pane",
            "-t",
            pane_id,
            "-p",
            "-S",
            &format!("-{lines}"),
        ])
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Refine activity detection by sniffing the last few lines of pane content.
/// Only does a capture_pane call when the title indicates "waiting" (not processing),
/// since processing state is fully determined by the title prefix.
pub async fn detect_activity_from_content(
    pane_id: &str,
    title_activity: crate::model::session::AgentActivity,
) -> crate::model::session::AgentActivity {
    use crate::model::session::AgentActivity;

    // Processing is definitive from title — no need to capture content
    if title_activity == AgentActivity::Processing {
        return AgentActivity::Processing;
    }

    // For waiting/unknown states, sniff content to detect permission prompts
    let content = match capture_pane(pane_id, 15).await {
        Ok(c) => c,
        Err(e) => {
            warn!(pane_id, error = %e, "failed to capture pane content");
            return title_activity;
        }
    };

    let last_lines: Vec<&str> = content.lines().rev().take(15).collect();

    // Check for permission/confirmation prompts
    let has_permission_prompt = last_lines.iter().any(|line| {
        line.contains("Do you want to proceed?")
            || line.contains("Yes, and don't ask again")
            || line.contains("Esc to cancel")
            || line.contains("Run shell command")
            || line.contains("approve this action")
    });

    if has_permission_prompt {
        return AgentActivity::WaitingForPermission;
    }

    title_activity
}

#[derive(Debug, Clone)]
pub struct AgentPaneInfo {
    pub tmux_ref: TmuxPaneRef,
    pub pid: u32,
    pub title: String,
    pub current_path: String,
    pub title_activity: crate::model::session::AgentActivity,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper that mimics the parsing logic in `list_all_panes` so we can
    /// test it without actually calling tmux.
    fn parse_tmux_output(output: &str) -> Vec<(String, u32, TmuxPane)> {
        let mut panes = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.split(SEP).collect();
            if parts.len() < 7 {
                continue;
            }
            let session_name = parts[0].to_string();
            let window_index: u32 = parts[1].parse().unwrap_or(0);
            let pane = TmuxPane {
                id: parts[2].to_string(),
                pid: parts[3].parse().unwrap_or(0),
                title: parts[4].to_string(),
                current_command: parts[5].to_string(),
                current_path: parts[6].to_string(),
            };
            panes.push((session_name, window_index, pane));
        }
        panes
    }

    #[test]
    fn parse_single_pane() {
        let output = "main|||0|||%1|||1234|||\u{2733} Claude|||node|||/home/user/project";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
        let (session, window, pane) = &panes[0];
        assert_eq!(session, "main");
        assert_eq!(*window, 0);
        assert_eq!(pane.id, "%1");
        assert_eq!(pane.pid, 1234);
        assert!(pane.title.contains("Claude"));
        assert_eq!(pane.current_command, "node");
        assert_eq!(pane.current_path, "/home/user/project");
    }

    #[test]
    fn parse_multiple_panes() {
        let output = "\
main|||0|||%1|||1234|||\u{2733} Claude|||node|||/home/user/project
work|||1|||%2|||5678|||bash|||bash|||/tmp
dev|||2|||%3|||9012|||\u{2801} Processing|||claude|||/home/user/other";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].0, "main");
        assert_eq!(panes[1].0, "work");
        assert_eq!(panes[2].0, "dev");
        assert!(panes[0].2.is_claude_agent());
        assert!(!panes[1].2.is_claude_agent());
        assert!(panes[2].2.is_claude_agent());
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let output = "\
main|||0|||%1|||1234|||\u{2733} Claude|||node|||/home/user/project
incomplete|||line
|||too|||few";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
    }

    #[test]
    fn parse_empty_output() {
        let panes = parse_tmux_output("");
        assert!(panes.is_empty());
    }

    #[test]
    fn parse_invalid_pid_defaults_to_zero() {
        let output = "main|||0|||%1|||not_a_number|||\u{2733} Claude|||node|||/tmp";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].2.pid, 0);
    }

    #[test]
    fn parse_invalid_window_index_defaults_to_zero() {
        let output = "main|||abc|||%1|||1234|||\u{2733} Claude|||node|||/tmp";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].1, 0);
    }

    #[test]
    fn parse_filters_agent_panes_correctly() {
        let output = "\
dev|||0|||%1|||100|||\u{2733} Idle agent|||claude|||/project
dev|||0|||%2|||200|||vim|||vim|||/project
dev|||1|||%3|||300|||\u{2840} Working|||claude|||/other";
        let panes = parse_tmux_output(output);
        let agents: Vec<_> = panes
            .iter()
            .filter(|(_, _, p)| p.is_claude_agent())
            .collect();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].2.id, "%1");
        assert_eq!(agents[1].2.id, "%3");
    }
}

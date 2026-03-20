use anyhow::Result;
use tokio::process::Command;

use crate::model::session::AgentType;
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

/// Detected agent pane with its type and metadata.
#[derive(Debug, Clone)]
pub struct AgentPaneInfo {
    pub tmux_ref: TmuxPaneRef,
    pub pid: u32,
    pub title: String,
    pub current_path: String,
    pub agent_type: AgentType,
    pub title_activity: crate::model::session::AgentActivity,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

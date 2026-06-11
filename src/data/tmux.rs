use std::collections::HashMap;

use anyhow::Result;
use tokio::process::Command;

use crate::model::session::AgentType;
use crate::model::tmux::{TmuxPane, TmuxPaneRef};

const SEP: &str = "|||";

/// A pane listed by tmux, with its window/session coordinates.
/// `window_id` is tmux's stable "@N" id — unlike `window_index`, it survives
/// window moves and renumbering, so it's the only safe rename/select target.
#[derive(Debug, Clone)]
pub struct PaneListing {
    pub session_name: String,
    pub window_index: u32,
    pub window_id: String,
    pub pane: TmuxPane,
}

/// List all panes across all tmux sessions in a single call.
///
/// "No tmux server" is a normal zero-agent state and returns an empty list;
/// any other failure is an error so callers don't mistake a transient tmux
/// problem for "all agents exited".
pub async fn list_all_panes() -> Result<Vec<PaneListing>> {
    let format_str = format!(
        "#{{session_name}}{SEP}#{{window_index}}{SEP}#{{window_id}}{SEP}#{{pane_id}}{SEP}#{{pane_pid}}{SEP}#{{pane_title}}{SEP}#{{pane_current_command}}{SEP}#{{pane_current_path}}{SEP}#{{pane_active}}"
    );

    let output = Command::new("tmux")
        .args(["list-panes", "-a", "-F", &format_str])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("error connecting to") {
            return Ok(Vec::new());
        }
        anyhow::bail!("tmux list-panes failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().filter_map(parse_pane_line).collect())
}

fn parse_pane_line(line: &str) -> Option<PaneListing> {
    let parts: Vec<&str> = line.split(SEP).collect();
    if parts.len() < 9 {
        return None;
    }

    Some(PaneListing {
        session_name: parts[0].to_string(),
        window_index: parts[1].parse().unwrap_or(0),
        window_id: parts[2].to_string(),
        pane: TmuxPane {
            id: parts[3].to_string(),
            pid: parts[4].parse().unwrap_or(0),
            title: parts[5].to_string(),
            current_command: parts[6].to_string(),
            current_path: parts[7].to_string(),
            active: parts[8] == "1",
        },
    })
}

/// Detected agent pane with its type and metadata.
#[derive(Debug, Clone)]
pub struct AgentPaneInfo {
    pub tmux_ref: TmuxPaneRef,
    /// Stable tmux window id ("@N") containing this pane.
    pub window_id: String,
    pub pid: u32,
    pub title: String,
    pub current_path: String,
    pub agent_type: AgentType,
    pub title_activity: crate::model::session::AgentActivity,
    /// Whether this pane is the active (focused) pane in its tmux window.
    pub pane_active: bool,
}

/// Window metadata needed for title management.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub name: String,
    /// Whether tmux's automatic-rename is currently enabled for this window.
    pub automatic_rename: bool,
}

/// Read all windows across all tmux sessions in a single call,
/// keyed by stable window id ("@N").
pub async fn get_all_windows() -> Result<HashMap<String, WindowInfo>> {
    let format_str =
        format!("#{{window_id}}{SEP}#{{window_name}}{SEP}#{{automatic-rename}}");
    let output = Command::new("tmux")
        .args(["list-windows", "-a", "-F", &format_str])
        .output()
        .await?;

    let mut map = HashMap::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(SEP).collect();
        if parts.len() >= 3 {
            map.insert(
                parts[0].to_string(),
                WindowInfo {
                    name: parts[1].to_string(),
                    automatic_rename: parts[2] == "1" || parts[2] == "on",
                },
            );
        }
    }
    Ok(map)
}

/// Run multiple tmux commands in a single invocation, chained with ';'.
pub async fn run_chained(commands: &[Vec<String>]) -> Result<()> {
    if commands.is_empty() {
        return Ok(());
    }

    let mut args: Vec<&str> = Vec::new();
    for (i, cmd) in commands.iter().enumerate() {
        if i > 0 {
            args.push(";");
        }
        args.extend(cmd.iter().map(|s| s.as_str()));
    }

    let output = Command::new("tmux").args(&args).output().await?;
    if !output.status.success() {
        anyhow::bail!(
            "tmux command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Send key names (tmux key syntax, e.g. "1", "Escape", "Enter") to a pane.
pub async fn send_keys(pane_id: &str, keys: &[String]) -> Result<()> {
    let mut cmd = vec![
        "send-keys".to_string(),
        "-t".to_string(),
        pane_id.to_string(),
    ];
    cmd.extend(keys.iter().cloned());
    run_chained(&[cmd]).await
}

/// Send literal text to a pane, then Enter to submit it.
/// The text is sent with `-l` so key-name lookup is disabled — the Enter
/// keypress goes in a separate command.
pub async fn send_text(pane_id: &str, text: &str) -> Result<()> {
    run_chained(&[
        vec![
            "send-keys".to_string(),
            "-t".to_string(),
            pane_id.to_string(),
            "-l".to_string(),
            "--".to_string(),
            text.to_string(),
        ],
        vec![
            "send-keys".to_string(),
            "-t".to_string(),
            pane_id.to_string(),
            "Enter".to_string(),
        ],
    ])
    .await
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

/// Capture pane content with ANSI escape sequences (`-e`) preserved, for
/// rendering with the pane's real colors. Use `capture_pane` for content
/// that will be string-matched.
pub async fn capture_pane_colored(pane_id: &str, lines: u32) -> Result<String> {
    let output = Command::new("tmux")
        .args([
            "capture-pane",
            "-t",
            pane_id,
            "-p",
            "-e",
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

    fn parse_tmux_output(output: &str) -> Vec<PaneListing> {
        output.lines().filter_map(parse_pane_line).collect()
    }

    #[test]
    fn parse_single_pane() {
        let output =
            "main|||0|||@7|||%1|||1234|||\u{2733} Claude|||node|||/home/user/project|||1";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
        let listing = &panes[0];
        assert_eq!(listing.session_name, "main");
        assert_eq!(listing.window_index, 0);
        assert_eq!(listing.window_id, "@7");
        assert_eq!(listing.pane.id, "%1");
        assert_eq!(listing.pane.pid, 1234);
        assert!(listing.pane.title.contains("Claude"));
        assert_eq!(listing.pane.current_command, "node");
        assert_eq!(listing.pane.current_path, "/home/user/project");
        assert!(listing.pane.active);
    }

    #[test]
    fn parse_multiple_panes() {
        let output = "\
main|||0|||@1|||%1|||1234|||\u{2733} Claude|||node|||/home/user/project|||1
work|||1|||@2|||%2|||5678|||bash|||bash|||/tmp|||0
dev|||2|||@3|||%3|||9012|||\u{2801} Processing|||claude|||/home/user/other|||1";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].session_name, "main");
        assert!(panes[0].pane.active);
        assert_eq!(panes[1].session_name, "work");
        assert_eq!(panes[1].window_id, "@2");
        assert!(!panes[1].pane.active);
        assert_eq!(panes[2].session_name, "dev");
        assert!(panes[2].pane.active);
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let output = "\
main|||0|||@1|||%1|||1234|||\u{2733} Claude|||node|||/home/user/project|||1
incomplete|||line
|||too|||few|||fields";
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
        let output = "main|||0|||@1|||%1|||not_a_number|||\u{2733} Claude|||node|||/tmp|||1";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].pane.pid, 0);
    }

    #[test]
    fn parse_invalid_window_index_defaults_to_zero() {
        let output = "main|||abc|||@1|||%1|||1234|||\u{2733} Claude|||node|||/tmp|||0";
        let panes = parse_tmux_output(output);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].window_index, 0);
        assert!(!panes[0].pane.active);
    }
}

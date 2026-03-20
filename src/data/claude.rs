use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;

use crate::model::history::HistoryEntry;

#[derive(Debug, Deserialize)]
pub struct ClaudeSessionFile {
    pub pid: Option<u32>,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub cwd: String,
    #[serde(rename = "startedAt")]
    pub started_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct SessionIndexEntry {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub modified: Option<i64>,
    #[serde(rename = "firstPrompt", default)]
    pub first_prompt: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(rename = "messageCount", default)]
    pub message_count: Option<u32>,
    #[serde(rename = "gitBranch", default)]
    pub git_branch: Option<String>,
}

/// Read all active session files from ~/.claude/sessions/
#[allow(dead_code)]
pub fn read_active_sessions(claude_dir: &Path) -> Result<Vec<ClaudeSessionFile>> {
    let sessions_dir = claude_dir.join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Ok(session) = serde_json::from_str::<ClaudeSessionFile>(&content) {
                    sessions.push(session);
                }
            }
            Err(_) => continue,
        }
    }

    Ok(sessions)
}

/// Read sessions-index.json from each project directory
pub fn read_project_sessions(claude_dir: &Path) -> Result<Vec<(String, Vec<SessionIndexEntry>)>> {
    let projects_dir = claude_dir.join("projects");
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut all_projects = Vec::new();

    for entry in std::fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Convert directory name back to path
        // e.g. "-Users-bcfd-mediait-ch-dev-swibrow-home-ops" -> "/Users/bcfd@mediait.ch/dev/swibrow/home-ops"
        let dir_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let project_path = decode_project_dir_name(&dir_name);

        let index_path = path.join("sessions-index.json");
        if !index_path.exists() {
            continue;
        }

        match std::fs::read_to_string(&index_path) {
            Ok(content) => {
                if let Ok(entries) = serde_json::from_str::<Vec<SessionIndexEntry>>(&content) {
                    all_projects.push((project_path, entries));
                }
            }
            Err(_) => continue,
        }
    }

    Ok(all_projects)
}

/// Read history.jsonl
pub fn read_history(claude_dir: &Path) -> Result<Vec<HistoryEntry>> {
    let history_path = claude_dir.join("history.jsonl");
    if !history_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&history_path)?;
    let mut entries = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Get list of all project directory paths
#[allow(dead_code)]
pub fn list_project_dirs(claude_dir: &Path) -> Result<Vec<PathBuf>> {
    let projects_dir = claude_dir.join("projects");
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(&projects_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            dirs.push(entry.path());
        }
    }

    Ok(dirs)
}

/// Decode a project directory name back to a filesystem path.
/// "-Users-bcfd-mediait-ch-dev-swibrow-home-ops" -> "/Users/bcfd@mediait.ch/dev/swibrow/home-ops"
/// This is a best-effort heuristic since the encoding isn't perfectly reversible.
fn decode_project_dir_name(name: &str) -> String {
    // The encoding replaces / with - and strips special chars
    // We just store the dir name as-is and use the project path from session files when available
    if name.starts_with('-') {
        name.replacen('-', "/", 1).replace('-', "/")
    } else {
        name.replace('-', "/")
    }
}

/// Extract project name from a path (last component)
pub fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── decode_project_dir_name ──────────────────────────────────

    #[test]
    fn decode_with_leading_dash() {
        let result = decode_project_dir_name("-Users-bcfd-dev-myproject");
        assert_eq!(result, "/Users/bcfd/dev/myproject");
    }

    #[test]
    fn decode_without_leading_dash() {
        let result = decode_project_dir_name("some-project-name");
        assert_eq!(result, "some/project/name");
    }

    #[test]
    fn decode_single_segment_no_dash() {
        let result = decode_project_dir_name("project");
        assert_eq!(result, "project");
    }

    #[test]
    fn decode_empty_string() {
        let result = decode_project_dir_name("");
        assert_eq!(result, "");
    }

    #[test]
    fn decode_just_a_dash() {
        let result = decode_project_dir_name("-");
        assert_eq!(result, "/");
    }

    #[test]
    fn decode_multiple_leading_dashes() {
        // Only the first dash becomes '/', the rest also become '/'
        let result = decode_project_dir_name("-a-b-c");
        assert_eq!(result, "/a/b/c");
    }

    // ── project_name_from_path ───────────────────────────────────

    #[test]
    fn project_name_simple_path() {
        assert_eq!(
            project_name_from_path("/Users/me/dev/cool-project"),
            "cool-project"
        );
    }

    #[test]
    fn project_name_trailing_slash() {
        // Path::new("/a/b/").file_name() returns Some("b")
        assert_eq!(project_name_from_path("/a/b/"), "b");
    }

    #[test]
    fn project_name_single_component() {
        assert_eq!(project_name_from_path("myproject"), "myproject");
    }

    #[test]
    fn project_name_root_path() {
        // Path::new("/").file_name() returns None
        assert_eq!(project_name_from_path("/"), "/");
    }

    #[test]
    fn project_name_empty() {
        assert_eq!(project_name_from_path(""), "");
    }

    // ── read_history (JSONL parsing) ─────────────────────────────

    #[test]
    fn read_history_parses_valid_lines() {
        let dir = tempfile::tempdir().unwrap();
        let history_path = dir.path().join("history.jsonl");
        let mut f = std::fs::File::create(&history_path).unwrap();
        writeln!(
            f,
            r#"{{"display":"first","timestamp":1000,"project":"/a"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"display":"second","timestamp":2000,"project":"/b","session_id":"s1"}}"#
        )
        .unwrap();

        let entries = read_history(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].display, "first");
        assert_eq!(entries[0].timestamp, 1000);
        assert_eq!(entries[0].project, "/a");
        assert!(entries[0].session_id.is_none());
        assert_eq!(entries[1].display, "second");
        assert_eq!(entries[1].session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn read_history_skips_empty_lines_and_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let history_path = dir.path().join("history.jsonl");
        let mut f = std::fs::File::create(&history_path).unwrap();
        writeln!(f, r#"{{"display":"good","timestamp":1000,"project":"/a"}}"#).unwrap();
        writeln!(f).unwrap(); // empty line
        writeln!(f, "not valid json").unwrap();
        writeln!(
            f,
            r#"{{"display":"also good","timestamp":2000,"project":"/b"}}"#
        )
        .unwrap();

        let entries = read_history(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn read_history_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let entries = read_history(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    // ── read_active_sessions ─────────────────────────────────────

    #[test]
    fn read_active_sessions_parses_json_files() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let session_json =
            r#"{"pid": 1234, "sessionId": "abc-123", "cwd": "/tmp", "startedAt": 999}"#;
        std::fs::write(sessions_dir.join("abc-123.json"), session_json).unwrap();

        let sessions = read_active_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "abc-123");
        assert_eq!(sessions[0].pid, Some(1234));
        assert_eq!(sessions[0].cwd, "/tmp");
        assert_eq!(sessions[0].started_at, 999);
    }

    #[test]
    fn read_active_sessions_skips_non_json_files() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        std::fs::write(sessions_dir.join("notes.txt"), "not json").unwrap();
        let session_json = r#"{"sessionId": "def-456", "cwd": "/home", "startedAt": 1000}"#;
        std::fs::write(sessions_dir.join("def-456.json"), session_json).unwrap();

        let sessions = read_active_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "def-456");
    }

    #[test]
    fn read_active_sessions_missing_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = read_active_sessions(dir.path()).unwrap();
        assert!(sessions.is_empty());
    }

    // ── read_project_sessions ────────────────────────────────────

    #[test]
    fn read_project_sessions_parses_index() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("projects").join("-Users-me-dev-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let index_json = r#"[{"sessionId": "s1", "created": 100, "modified": 200, "firstPrompt": "hello", "messageCount": 5}]"#;
        std::fs::write(project_dir.join("sessions-index.json"), index_json).unwrap();

        let projects = read_project_sessions(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        let (path, entries) = &projects[0];
        assert!(path.starts_with("/Users"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_id, "s1");
        assert_eq!(entries[0].first_prompt.as_deref(), Some("hello"));
        assert_eq!(entries[0].message_count, Some(5));
    }

    #[test]
    fn read_project_sessions_missing_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let projects = read_project_sessions(dir.path()).unwrap();
        assert!(projects.is_empty());
    }
}

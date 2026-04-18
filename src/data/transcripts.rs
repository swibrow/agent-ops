//! Shared transcript parsing + DB-backed ingest.
//!
//! Currently Claude-only. See `ingest_transcripts` on the `AgentProvider` trait
//! for how other agents plug in.
//!
//! Claude JSONL format lives at `~/.claude/projects/<encoded-project>/<sessionId>.jsonl`
//! and stores one JSON object per line. Object types: user, assistant, system,
//! file-history-snapshot, etc. We keep only user and assistant messages.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;
use tracing::{debug, warn};

/// A parsed chat message ready to be inserted into the DB or returned via API.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedMessage {
    pub uuid: String,
    pub session_id: String,
    pub role: String,
    pub content: serde_json::Value,
    /// Unix milliseconds
    pub timestamp: i64,
    pub project_path: String,
    /// Token usage — only non-zero for assistant messages (Claude).
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

/// Parse a single Claude JSONL line into a `ParsedMessage`, if applicable.
/// Returns None for lines we skip (system messages, file snapshots, etc.) or
/// invalid JSON.
pub fn parse_claude_line(line: &str) -> Option<ParsedMessage> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let msg_type = value.get("type")?.as_str()?;
    if msg_type != "user" && msg_type != "assistant" {
        return None;
    }

    let uuid = value.get("uuid")?.as_str()?.to_string();
    let session_id = value.get("sessionId")?.as_str()?.to_string();
    let cwd = value
        .get("cwd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let timestamp_str = value.get("timestamp")?.as_str()?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
        .ok()?
        .timestamp_millis();

    // Content is nested: { message: { role, content } }
    let message = value.get("message")?;
    let role = message
        .get("role")
        .and_then(|r| r.as_str())
        .unwrap_or(msg_type)
        .to_string();
    let content = message.get("content").cloned().unwrap_or(serde_json::Value::Null);

    // Extract token usage from assistant messages (Claude-specific field).
    let (input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens) =
        if msg_type == "assistant" {
            let u = message.get("usage");
            (
                u.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_i64()).unwrap_or(0),
                u.and_then(|u| u.get("output_tokens")).and_then(|v| v.as_i64()).unwrap_or(0),
                u.and_then(|u| u.get("cache_creation_input_tokens")).and_then(|v| v.as_i64()).unwrap_or(0),
                u.and_then(|u| u.get("cache_read_input_tokens")).and_then(|v| v.as_i64()).unwrap_or(0),
            )
        } else {
            (0, 0, 0, 0)
        };

    Some(ParsedMessage {
        uuid,
        session_id,
        role,
        content,
        timestamp,
        project_path: cwd,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    })
}

/// Parse an entire Claude JSONL file.
pub fn parse_claude_jsonl(content: &str) -> Vec<ParsedMessage> {
    content
        .lines()
        .filter_map(|line| {
            if line.trim().is_empty() {
                None
            } else {
                parse_claude_line(line)
            }
        })
        .collect()
}

/// Insert a batch of messages into the DB. Uses INSERT OR IGNORE so re-ingesting
/// the same file is idempotent (uuid is the primary key).
pub fn insert_messages(
    conn: &Connection,
    messages: &[ParsedMessage],
    agent_type: &str,
) -> Result<usize> {
    if messages.is_empty() {
        return Ok(0);
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO messages
            (uuid, session_id, agent_type, role, content, timestamp, project_path,
             input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(uuid) DO UPDATE SET
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            cache_read_tokens = excluded.cache_read_tokens",
    )?;
    let mut inserted = 0;
    for m in messages {
        let changed = stmt.execute(params![
            m.uuid,
            m.session_id,
            agent_type,
            m.role,
            m.content.to_string(),
            m.timestamp,
            m.project_path,
            m.input_tokens,
            m.output_tokens,
            m.cache_creation_tokens,
            m.cache_read_tokens,
        ])?;
        inserted += changed;
    }
    Ok(inserted)
}

/// Record that we've fully ingested `path`, so we can skip it next time unless
/// mtime or size changes.
pub fn record_ingest_state(
    conn: &Connection,
    path: &Path,
    agent_type: &str,
    mtime: i64,
    size: i64,
    message_count: usize,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ingest_state (file_path, agent_type, mtime, size, message_count, last_ingested)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(file_path) DO UPDATE SET
            mtime = excluded.mtime,
            size = excluded.size,
            message_count = excluded.message_count,
            last_ingested = excluded.last_ingested",
        params![
            path.to_string_lossy(),
            agent_type,
            mtime,
            size,
            message_count as i64,
            chrono::Utc::now().timestamp_millis(),
        ],
    )?;
    Ok(())
}

/// Check whether a file needs re-ingesting based on mtime and size.
/// Returns true if the file changed since we last ingested (or we never did).
pub fn needs_ingest(conn: &Connection, path: &Path, mtime: i64, size: i64) -> Result<bool> {
    let row: Option<(i64, i64)> = conn
        .query_row(
            "SELECT mtime, size FROM ingest_state WHERE file_path = ?1",
            params![path.to_string_lossy()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    match row {
        Some((known_mtime, known_size)) => Ok(known_mtime < mtime || known_size != size),
        None => Ok(true),
    }
}

/// Ingest a single Claude JSONL file into the DB. Idempotent and cheap on
/// repeat (skipped if unchanged). Returns the number of newly inserted messages.
pub fn ingest_claude_file(conn: &Connection, path: &Path) -> Result<usize> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let size = meta.len() as i64;

    if !needs_ingest(conn, path, mtime, size)? {
        return Ok(0);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let messages = parse_claude_jsonl(&content);
    let inserted = insert_messages(conn, &messages, "claude")?;

    record_ingest_state(conn, path, "claude", mtime, size, messages.len())?;

    if inserted > 0 {
        debug!(
            path = %path.display(),
            total = messages.len(),
            new = inserted,
            "ingested transcript"
        );
    }
    Ok(inserted)
}

/// Walk every `<claude_dir>/projects/*/*.jsonl` file and ingest it.
/// Returns total messages newly inserted across all files.
pub fn ingest_all_claude_dirs(conn: &Connection, claude_dirs: &[std::path::PathBuf]) -> Result<usize> {
    let mut total = 0;
    for claude_dir in claude_dirs {
        let projects_dir = claude_dir.join("projects");
        if !projects_dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(&projects_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(dir = %projects_dir.display(), error = %e, "read_dir failed");
                continue;
            }
        };
        for entry in entries.flatten() {
            let project = entry.path();
            if !project.is_dir() {
                continue;
            }
            let jsonl_entries = match std::fs::read_dir(&project) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for file in jsonl_entries.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                match ingest_claude_file(conn, &path) {
                    Ok(n) => total += n,
                    Err(e) => warn!(path = %path.display(), error = %e, "ingest failed"),
                }
            }
        }
    }
    Ok(total)
}

/// Delete messages older than `retention_days`. Returns the number of rows deleted.
/// If `retention_days` is None, does nothing.
pub fn prune_old_messages(conn: &Connection, retention_days: Option<u32>) -> Result<usize> {
    let Some(days) = retention_days else {
        return Ok(0);
    };
    if days == 0 {
        return Ok(0);
    }
    let cutoff = chrono::Utc::now().timestamp_millis() - (days as i64 * 86_400_000);
    let deleted = conn.execute(
        "DELETE FROM messages WHERE timestamp < ?1",
        params![cutoff],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::run_migrations;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn parse_user_message() {
        let line = r#"{"type":"user","uuid":"u1","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:00Z","message":{"role":"user","content":"hello"}}"#;
        let msg = parse_claude_line(line).unwrap();
        assert_eq!(msg.uuid, "u1");
        assert_eq!(msg.session_id, "s1");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.project_path, "/p");
        assert_eq!(msg.content.as_str(), Some("hello"));
    }

    #[test]
    fn parse_assistant_message_with_content_blocks() {
        let line = r#"{"type":"assistant","uuid":"a1","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"hi there"}]}}"#;
        let msg = parse_claude_line(line).unwrap();
        assert_eq!(msg.role, "assistant");
        assert!(msg.content.is_array());
        // No usage field → all zeros
        assert_eq!(msg.input_tokens, 0);
        assert_eq!(msg.output_tokens, 0);
    }

    #[test]
    fn parse_assistant_message_extracts_token_usage() {
        let line = r#"{"type":"assistant","uuid":"a2","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:06Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":1500}}}"#;
        let msg = parse_claude_line(line).unwrap();
        assert_eq!(msg.input_tokens, 100);
        assert_eq!(msg.output_tokens, 50);
        assert_eq!(msg.cache_creation_tokens, 200);
        assert_eq!(msg.cache_read_tokens, 1500);
    }

    #[test]
    fn parse_user_message_has_zero_tokens() {
        let line = r#"{"type":"user","uuid":"u1","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:00Z","message":{"role":"user","content":"hello"}}"#;
        let msg = parse_claude_line(line).unwrap();
        assert_eq!(msg.input_tokens, 0);
        assert_eq!(msg.output_tokens, 0);
    }

    #[test]
    fn parse_skips_non_message_types() {
        let line = r#"{"type":"file-history-snapshot","uuid":"x","sessionId":"s","cwd":"/p","timestamp":"2026-04-08T10:00:00Z"}"#;
        assert!(parse_claude_line(line).is_none());
    }

    #[test]
    fn parse_skips_invalid_json() {
        assert!(parse_claude_line("not json").is_none());
    }

    #[test]
    fn insert_messages_is_idempotent() {
        let conn = setup_db();
        let msg = ParsedMessage {
            uuid: "u1".to_string(),
            session_id: "s1".to_string(),
            role: "user".to_string(),
            content: serde_json::Value::String("hello".to_string()),
            timestamp: 1000,
            project_path: "/p".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        let batch = vec![msg.clone()];
        assert_eq!(insert_messages(&conn, &batch, "claude").unwrap(), 1);
        // Re-insert same uuid: upsert updates token columns (returns 1, not duplicate)
        insert_messages(&conn, &batch, "claude").unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn ingest_file_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc-123.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"user","uuid":"u1","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:00Z","message":{"role":"user","content":"hi"}}"#,
                "\n",
                r#"{"type":"assistant","uuid":"a1","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}"#,
                "\n",
                r#"{"type":"system","uuid":"sys1","sessionId":"s1","cwd":"/p","timestamp":"2026-04-08T10:00:10Z"}"#,
                "\n",
            ),
        )
        .unwrap();

        let conn = setup_db();
        let inserted = ingest_claude_file(&conn, &path).unwrap();
        assert_eq!(inserted, 2); // user + assistant, not system

        // Re-ingest: nothing new (mtime unchanged)
        let inserted = ingest_claude_file(&conn, &path).unwrap();
        assert_eq!(inserted, 0);
    }

    #[test]
    fn prune_respects_retention() {
        let conn = setup_db();
        let old_ts = chrono::Utc::now().timestamp_millis() - (10 * 86_400_000);
        let fresh_ts = chrono::Utc::now().timestamp_millis();
        let batch = vec![
            ParsedMessage {
                uuid: "old".to_string(),
                session_id: "s".to_string(),
                role: "user".to_string(),
                content: serde_json::Value::String("old".to_string()),
                timestamp: old_ts,
                project_path: "/p".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            ParsedMessage {
                uuid: "fresh".to_string(),
                session_id: "s".to_string(),
                role: "user".to_string(),
                content: serde_json::Value::String("fresh".to_string()),
                timestamp: fresh_ts,
                project_path: "/p".to_string(),
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
        ];
        insert_messages(&conn, &batch, "claude").unwrap();

        let deleted = prune_old_messages(&conn, Some(7)).unwrap();
        assert_eq!(deleted, 1);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn prune_none_is_noop() {
        let conn = setup_db();
        assert_eq!(prune_old_messages(&conn, None).unwrap(), 0);
        assert_eq!(prune_old_messages(&conn, Some(0)).unwrap(), 0);
    }
}

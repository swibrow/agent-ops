use anyhow::Result;
use rusqlite::{params, Connection};

use crate::model::history::HistoryEntry;
use crate::model::project::{Project, StalenessLevel};
use crate::model::session::{AgentActivity, AgentSession, SessionStatus};
use crate::model::tmux::TmuxPaneRef;

pub fn upsert_session(conn: &Connection, session: &AgentSession) -> Result<()> {
    conn.execute(
        "INSERT INTO sessions (session_id, pid, project_path, project_name, started_at, ended_at, status, first_prompt, summary, git_branch, message_count, last_activity, tmux_session, tmux_window, tmux_pane_id, pane_title)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(session_id) DO UPDATE SET
            pid = COALESCE(excluded.pid, sessions.pid),
            status = CASE
                WHEN excluded.status = 'active' THEN 'active'
                WHEN sessions.status = 'active' THEN sessions.status
                ELSE excluded.status
            END,
            ended_at = CASE
                WHEN excluded.status = 'active' THEN NULL
                ELSE COALESCE(excluded.ended_at, sessions.ended_at)
            END,
            first_prompt = COALESCE(excluded.first_prompt, sessions.first_prompt),
            summary = COALESCE(excluded.summary, sessions.summary),
            git_branch = COALESCE(excluded.git_branch, sessions.git_branch),
            message_count = MAX(excluded.message_count, sessions.message_count),
            last_activity = CASE
                WHEN excluded.status = 'active' THEN excluded.last_activity
                ELSE COALESCE(sessions.last_activity, excluded.last_activity)
            END,
            tmux_session = COALESCE(excluded.tmux_session, sessions.tmux_session),
            tmux_window = COALESCE(excluded.tmux_window, sessions.tmux_window),
            tmux_pane_id = COALESCE(excluded.tmux_pane_id, sessions.tmux_pane_id),
            pane_title = COALESCE(excluded.pane_title, sessions.pane_title)",
        params![
            session.session_id,
            session.pid,
            session.project_path,
            session.project_name,
            session.started_at,
            session.ended_at,
            session.status.as_str(),
            session.first_prompt,
            session.summary,
            session.git_branch,
            session.message_count,
            session.last_activity,
            session.tmux_pane.as_ref().map(|p| &p.session_name),
            session.tmux_pane.as_ref().map(|p| p.window_index),
            session.tmux_pane.as_ref().map(|p| &p.pane_id),
            session.pane_title,
        ],
    )?;
    Ok(())
}

pub fn upsert_project(conn: &Connection, project: &Project) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (path, name, first_seen, last_activity, total_sessions, total_messages, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(path) DO UPDATE SET
            last_activity = MAX(excluded.last_activity, projects.last_activity),
            total_sessions = excluded.total_sessions,
            total_messages = excluded.total_messages,
            is_active = excluded.is_active",
        params![
            project.path,
            project.name,
            project.first_seen,
            project.last_activity,
            project.total_sessions,
            project.total_messages,
            project.is_active,
        ],
    )?;
    Ok(())
}

pub fn insert_history(conn: &Connection, entry: &HistoryEntry) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO history (timestamp, project_path, display, session_id)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            entry.timestamp,
            entry.project,
            entry.display,
            entry.session_id
        ],
    )?;
    Ok(())
}

pub fn get_all_sessions(conn: &Connection) -> Result<Vec<AgentSession>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, pid, project_path, project_name, started_at, ended_at, status, first_prompt, summary, git_branch, message_count, last_activity, tmux_session, tmux_window, tmux_pane_id, pane_title
         FROM sessions ORDER BY last_activity DESC NULLS LAST"
    )?;

    let sessions = stmt
        .query_map([], |row| {
            let tmux_session: Option<String> = row.get(12)?;
            let tmux_window: Option<u32> = row.get(13)?;
            let tmux_pane_id: Option<String> = row.get(14)?;

            let tmux_pane = match (tmux_session, tmux_window, tmux_pane_id) {
                (Some(s), Some(w), Some(p)) => Some(TmuxPaneRef {
                    session_name: s,
                    window_index: w,
                    pane_id: p,
                }),
                _ => None,
            };

            Ok(AgentSession {
                session_id: row.get(0)?,
                pid: row.get(1)?,
                project_path: row.get(2)?,
                project_name: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: SessionStatus::from_str(row.get::<_, String>(6)?.as_str()),
                first_prompt: row.get(7)?,
                summary: row.get(8)?,
                git_branch: row.get(9)?,
                message_count: row.get(10)?,
                last_activity: row.get(11)?,
                tmux_pane,
                pane_title: row.get(15)?,
                activity: AgentActivity::Unknown,
                cpu_percent: 0.0,
                memory_mb: 0.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(sessions)
}

pub fn get_active_sessions(conn: &Connection) -> Result<Vec<AgentSession>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, pid, project_path, project_name, started_at, ended_at, status, first_prompt, summary, git_branch, message_count, last_activity, tmux_session, tmux_window, tmux_pane_id, pane_title
         FROM sessions WHERE status IN ('active', 'idle') ORDER BY last_activity DESC"
    )?;

    let sessions = stmt
        .query_map([], |row| {
            let tmux_session: Option<String> = row.get(12)?;
            let tmux_window: Option<u32> = row.get(13)?;
            let tmux_pane_id: Option<String> = row.get(14)?;

            let tmux_pane = match (tmux_session, tmux_window, tmux_pane_id) {
                (Some(s), Some(w), Some(p)) => Some(TmuxPaneRef {
                    session_name: s,
                    window_index: w,
                    pane_id: p,
                }),
                _ => None,
            };

            Ok(AgentSession {
                session_id: row.get(0)?,
                pid: row.get(1)?,
                project_path: row.get(2)?,
                project_name: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: SessionStatus::from_str(row.get::<_, String>(6)?.as_str()),
                first_prompt: row.get(7)?,
                summary: row.get(8)?,
                git_branch: row.get(9)?,
                message_count: row.get(10)?,
                last_activity: row.get(11)?,
                tmux_pane,
                pane_title: row.get(15)?,
                activity: AgentActivity::Unknown,
                cpu_percent: 0.0,
                memory_mb: 0.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(sessions)
}

pub fn get_all_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT path, name, first_seen, last_activity, total_sessions, total_messages, is_active
         FROM projects ORDER BY last_activity DESC",
    )?;

    let now = chrono::Utc::now().timestamp() * 1000;

    let projects = stmt
        .query_map([], |row| {
            let last_activity: i64 = row.get(3)?;
            let hours_since = (now - last_activity) / (1000 * 3600);
            let is_active = row.get::<_, i32>(6)? != 0;

            Ok(Project {
                path: row.get(0)?,
                name: row.get(1)?,
                first_seen: row.get(2)?,
                last_activity,
                total_sessions: row.get(4)?,
                total_messages: row.get(5)?,
                is_active,
                staleness: if is_active {
                    StalenessLevel::Hot
                } else {
                    StalenessLevel::from_hours(hours_since)
                },
                daily_activity: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(projects)
}

pub fn get_daily_activity(conn: &Connection, project_path: &str, days: u32) -> Result<Vec<u64>> {
    let mut stmt = conn.prepare(
        "SELECT message_count FROM daily_activity
         WHERE project_path = ?1
         ORDER BY date DESC LIMIT ?2",
    )?;

    let mut counts: Vec<u64> = stmt
        .query_map(params![project_path, days], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    counts.reverse(); // oldest first for sparkline
                      // Pad to full length
    while counts.len() < days as usize {
        counts.insert(0, 0);
    }

    Ok(counts)
}

pub fn get_history(
    conn: &Connection,
    limit: u32,
    project_filter: Option<&str>,
) -> Result<Vec<HistoryEntry>> {
    let (sql, filter_params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match project_filter {
        Some(project) => (
            "SELECT timestamp, project_path, display, session_id FROM history WHERE project_path LIKE ?1 ORDER BY timestamp DESC LIMIT ?2".to_string(),
            vec![Box::new(format!("%{project}%")), Box::new(limit)],
        ),
        None => (
            "SELECT timestamp, project_path, display, session_id FROM history ORDER BY timestamp DESC LIMIT ?1".to_string(),
            vec![Box::new(limit)],
        ),
    };

    let mut stmt = conn.prepare(&sql)?;
    let entries = stmt
        .query_map(rusqlite::params_from_iter(filter_params.iter()), |row| {
            Ok(HistoryEntry {
                timestamp: row.get(0)?,
                project: row.get(1)?,
                display: row.get(2)?,
                session_id: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(entries)
}

pub fn mark_sessions_completed(conn: &Connection, active_pane_ids: &[String]) -> Result<usize> {
    if active_pane_ids.is_empty() {
        let changed = conn.execute(
            "UPDATE sessions SET status = 'completed', ended_at = ?1 WHERE status = 'active'",
            params![chrono::Utc::now().timestamp_millis()],
        )?;
        return Ok(changed);
    }

    let placeholders: Vec<String> = (0..active_pane_ids.len())
        .map(|i| format!("?{}", i + 2))
        .collect();
    let sql = format!(
        "UPDATE sessions SET status = 'completed', ended_at = ?1 WHERE status = 'active' AND tmux_pane_id NOT IN ({})",
        placeholders.join(",")
    );

    let now = chrono::Utc::now().timestamp_millis();
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];
    for id in active_pane_ids {
        all_params.push(Box::new(id.clone()));
    }

    let changed = conn.execute(&sql, rusqlite::params_from_iter(all_params.iter()))?;
    Ok(changed)
}

/// Rebuild daily_activity table from history table
pub fn rebuild_daily_activity(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM daily_activity;
         INSERT INTO daily_activity (project_path, date, message_count, session_count)
         SELECT
            project_path,
            DATE(timestamp / 1000, 'unixepoch') as date,
            COUNT(*) as message_count,
            COUNT(DISTINCT session_id) as session_count
         FROM history
         GROUP BY project_path, date;",
    )?;
    Ok(())
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

    fn make_session(id: &str, project_path: &str, status: SessionStatus) -> AgentSession {
        AgentSession {
            session_id: id.to_string(),
            pid: Some(1234),
            project_path: project_path.to_string(),
            project_name: project_path
                .rsplit('/')
                .next()
                .unwrap_or(project_path)
                .to_string(),
            started_at: 1000,
            ended_at: None,
            status,
            first_prompt: Some("do something".to_string()),
            summary: None,
            git_branch: Some("main".to_string()),
            message_count: 5,
            last_activity: Some(2000),
            tmux_pane: Some(TmuxPaneRef {
                session_name: "dev".to_string(),
                window_index: 0,
                pane_id: "%1".to_string(),
            }),
            pane_title: Some("Claude".to_string()),
            activity: AgentActivity::Unknown,
            cpu_percent: 0.0,
            memory_mb: 0.0,
        }
    }

    // ── upsert_session: insert ───────────────────────────────────

    #[test]
    fn upsert_session_inserts_new() {
        let conn = setup_db();
        let session = make_session("s1", "/tmp/project", SessionStatus::Active);
        upsert_session(&conn, &session).unwrap();

        let sessions = get_all_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s1");
        assert_eq!(sessions[0].project_path, "/tmp/project");
        assert_eq!(sessions[0].status, SessionStatus::Active);
        assert_eq!(sessions[0].first_prompt.as_deref(), Some("do something"));
        assert_eq!(sessions[0].git_branch.as_deref(), Some("main"));
        assert_eq!(sessions[0].message_count, 5);
    }

    // ── upsert_session: update ───────────────────────────────────

    #[test]
    fn upsert_session_updates_existing() {
        let conn = setup_db();

        // Insert initial
        let session = make_session("s1", "/tmp/project", SessionStatus::Active);
        upsert_session(&conn, &session).unwrap();

        // Update with higher message count
        let mut updated = make_session("s1", "/tmp/project", SessionStatus::Active);
        updated.message_count = 10;
        updated.summary = Some("did stuff".to_string());
        upsert_session(&conn, &updated).unwrap();

        let sessions = get_all_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 10); // MAX
        assert_eq!(sessions[0].summary.as_deref(), Some("did stuff")); // COALESCE
    }

    #[test]
    fn upsert_session_active_overrides_non_active_status() {
        let conn = setup_db();

        // Insert as completed
        let session = make_session("s1", "/tmp/project", SessionStatus::Completed);
        upsert_session(&conn, &session).unwrap();

        // Update with active status
        let active = make_session("s1", "/tmp/project", SessionStatus::Active);
        upsert_session(&conn, &active).unwrap();

        let sessions = get_all_sessions(&conn).unwrap();
        assert_eq!(sessions[0].status, SessionStatus::Active);
    }

    #[test]
    fn upsert_session_preserves_active_status_when_completed_arrives() {
        let conn = setup_db();

        // Insert as active
        let session = make_session("s1", "/tmp/project", SessionStatus::Active);
        upsert_session(&conn, &session).unwrap();

        // Try to complete it via upsert (not mark_sessions_completed)
        let completed = make_session("s1", "/tmp/project", SessionStatus::Completed);
        upsert_session(&conn, &completed).unwrap();

        let sessions = get_all_sessions(&conn).unwrap();
        // Status should remain active per the CASE logic
        assert_eq!(sessions[0].status, SessionStatus::Active);
    }

    #[test]
    fn upsert_session_coalesces_first_prompt() {
        let conn = setup_db();

        // Insert with first_prompt
        let session = make_session("s1", "/tmp/project", SessionStatus::Active);
        upsert_session(&conn, &session).unwrap();

        // Update without first_prompt (None)
        let mut updated = make_session("s1", "/tmp/project", SessionStatus::Active);
        updated.first_prompt = None;
        upsert_session(&conn, &updated).unwrap();

        let sessions = get_all_sessions(&conn).unwrap();
        // Original first_prompt should be preserved
        assert_eq!(sessions[0].first_prompt.as_deref(), Some("do something"));
    }

    // ── get_active_sessions ──────────────────────────────────────

    #[test]
    fn get_active_sessions_filters_correctly() {
        let conn = setup_db();

        upsert_session(&conn, &make_session("s1", "/a", SessionStatus::Active)).unwrap();
        upsert_session(&conn, &make_session("s2", "/b", SessionStatus::Completed)).unwrap();
        upsert_session(&conn, &make_session("s3", "/c", SessionStatus::Idle)).unwrap();

        let active = get_active_sessions(&conn).unwrap();
        assert_eq!(active.len(), 2);
        let ids: Vec<&str> = active.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids.contains(&"s1"));
        assert!(ids.contains(&"s3"));
    }

    // ── mark_sessions_completed ──────────────────────────────────

    #[test]
    fn mark_sessions_completed_with_empty_active_panes() {
        let conn = setup_db();

        upsert_session(&conn, &make_session("s1", "/a", SessionStatus::Active)).unwrap();
        upsert_session(&conn, &make_session("s2", "/b", SessionStatus::Active)).unwrap();

        let changed = mark_sessions_completed(&conn, &[]).unwrap();
        assert_eq!(changed, 2);

        let sessions = get_all_sessions(&conn).unwrap();
        for s in &sessions {
            assert_eq!(s.status, SessionStatus::Completed);
            assert!(s.ended_at.is_some());
        }
    }

    #[test]
    fn mark_sessions_completed_preserves_active_panes() {
        let conn = setup_db();

        // s1 has pane_id %1, s2 has pane_id %2
        let mut s1 = make_session("s1", "/a", SessionStatus::Active);
        s1.tmux_pane = Some(TmuxPaneRef {
            session_name: "dev".to_string(),
            window_index: 0,
            pane_id: "%1".to_string(),
        });
        upsert_session(&conn, &s1).unwrap();

        let mut s2 = make_session("s2", "/b", SessionStatus::Active);
        s2.tmux_pane = Some(TmuxPaneRef {
            session_name: "dev".to_string(),
            window_index: 1,
            pane_id: "%2".to_string(),
        });
        upsert_session(&conn, &s2).unwrap();

        // Only %1 is still active
        let changed = mark_sessions_completed(&conn, &["%1".to_string()]).unwrap();
        assert_eq!(changed, 1); // Only s2 should be completed

        let sessions = get_all_sessions(&conn).unwrap();
        let s1_db = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        let s2_db = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s1_db.status, SessionStatus::Active);
        assert_eq!(s2_db.status, SessionStatus::Completed);
    }

    #[test]
    fn mark_sessions_completed_ignores_already_completed() {
        let conn = setup_db();

        upsert_session(&conn, &make_session("s1", "/a", SessionStatus::Completed)).unwrap();

        let changed = mark_sessions_completed(&conn, &[]).unwrap();
        assert_eq!(changed, 0);
    }

    // ── get_daily_activity ───────────────────────────────────────

    #[test]
    fn get_daily_activity_returns_padded_results() {
        let conn = setup_db();

        // Insert some daily activity
        conn.execute(
            "INSERT INTO daily_activity (project_path, date, message_count, session_count) VALUES (?1, ?2, ?3, ?4)",
            params!["/project", "2026-03-17", 10, 2],
        ).unwrap();
        conn.execute(
            "INSERT INTO daily_activity (project_path, date, message_count, session_count) VALUES (?1, ?2, ?3, ?4)",
            params!["/project", "2026-03-16", 5, 1],
        ).unwrap();

        let activity = get_daily_activity(&conn, "/project", 7).unwrap();
        assert_eq!(activity.len(), 7); // padded to 7
                                       // First 5 should be zeros (padding), then real data
        assert_eq!(activity[0], 0);
        assert_eq!(activity[1], 0);
        assert_eq!(activity[2], 0);
        assert_eq!(activity[3], 0);
        assert_eq!(activity[4], 0);
        // The last two should be the real values (reversed from DESC to oldest-first)
        assert_eq!(activity[5], 5);
        assert_eq!(activity[6], 10);
    }

    #[test]
    fn get_daily_activity_no_data_returns_all_zeros() {
        let conn = setup_db();
        let activity = get_daily_activity(&conn, "/nonexistent", 5).unwrap();
        assert_eq!(activity.len(), 5);
        assert!(activity.iter().all(|&v| v == 0));
    }

    // ── insert_history + get_history ─────────────────────────────

    #[test]
    fn insert_and_get_history() {
        let conn = setup_db();

        let entry1 = HistoryEntry {
            timestamp: 1000,
            project: "/project-a".to_string(),
            display: "first command".to_string(),
            session_id: Some("s1".to_string()),
        };
        let entry2 = HistoryEntry {
            timestamp: 2000,
            project: "/project-b".to_string(),
            display: "second command".to_string(),
            session_id: None,
        };

        insert_history(&conn, &entry1).unwrap();
        insert_history(&conn, &entry2).unwrap();

        // Get all
        let all = get_history(&conn, 100, None).unwrap();
        assert_eq!(all.len(), 2);
        // Most recent first
        assert_eq!(all[0].timestamp, 2000);
        assert_eq!(all[1].timestamp, 1000);
    }

    #[test]
    fn get_history_with_project_filter() {
        let conn = setup_db();

        insert_history(
            &conn,
            &HistoryEntry {
                timestamp: 1000,
                project: "/Users/me/project-alpha".to_string(),
                display: "alpha cmd".to_string(),
                session_id: None,
            },
        )
        .unwrap();
        insert_history(
            &conn,
            &HistoryEntry {
                timestamp: 2000,
                project: "/Users/me/project-beta".to_string(),
                display: "beta cmd".to_string(),
                session_id: None,
            },
        )
        .unwrap();

        let filtered = get_history(&conn, 100, Some("alpha")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].display, "alpha cmd");
    }

    #[test]
    fn get_history_respects_limit() {
        let conn = setup_db();

        for i in 0..10 {
            insert_history(
                &conn,
                &HistoryEntry {
                    timestamp: i * 1000,
                    project: "/project".to_string(),
                    display: format!("cmd {i}"),
                    session_id: None,
                },
            )
            .unwrap();
        }

        let limited = get_history(&conn, 3, None).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn insert_history_ignores_duplicate() {
        let conn = setup_db();

        let entry = HistoryEntry {
            timestamp: 1000,
            project: "/project".to_string(),
            display: "cmd".to_string(),
            session_id: None,
        };

        insert_history(&conn, &entry).unwrap();
        insert_history(&conn, &entry).unwrap(); // duplicate (same timestamp + project)

        let all = get_history(&conn, 100, None).unwrap();
        assert_eq!(all.len(), 1);
    }

    // ── rebuild_daily_activity ───────────────────────────────────

    #[test]
    fn rebuild_daily_activity_from_history() {
        let conn = setup_db();

        // Insert history entries on the same day (2026-03-17 in UTC)
        // 1742169600000 = 2026-03-17T00:00:00Z in milliseconds
        let base_ts = 1742169600_i64 * 1000;
        insert_history(
            &conn,
            &HistoryEntry {
                timestamp: base_ts,
                project: "/project".to_string(),
                display: "cmd 1".to_string(),
                session_id: Some("s1".to_string()),
            },
        )
        .unwrap();
        insert_history(
            &conn,
            &HistoryEntry {
                timestamp: base_ts + 1000,
                project: "/project".to_string(),
                display: "cmd 2".to_string(),
                session_id: Some("s1".to_string()),
            },
        )
        .unwrap();
        insert_history(
            &conn,
            &HistoryEntry {
                timestamp: base_ts + 2000,
                project: "/project".to_string(),
                display: "cmd 3".to_string(),
                session_id: Some("s2".to_string()),
            },
        )
        .unwrap();

        rebuild_daily_activity(&conn).unwrap();

        // Should have 1 row: 3 messages, 2 sessions
        let msg_count: i64 = conn
            .query_row(
                "SELECT message_count FROM daily_activity WHERE project_path = '/project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(msg_count, 3);

        let session_count: i64 = conn
            .query_row(
                "SELECT session_count FROM daily_activity WHERE project_path = '/project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_count, 2);
    }

    #[test]
    fn rebuild_daily_activity_clears_old_data() {
        let conn = setup_db();

        // Manually insert stale data
        conn.execute(
            "INSERT INTO daily_activity (project_path, date, message_count) VALUES ('stale', '2020-01-01', 99)",
            [],
        ).unwrap();

        // Rebuild with no history -> stale row should be gone
        rebuild_daily_activity(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_activity", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // ── upsert_project + get_all_projects ────────────────────────

    #[test]
    fn upsert_and_get_projects() {
        let conn = setup_db();

        let now = chrono::Utc::now().timestamp_millis();
        let project = Project {
            path: "/Users/me/myproject".to_string(),
            name: "myproject".to_string(),
            first_seen: now - 100_000,
            last_activity: now,
            total_sessions: 3,
            total_messages: 15,
            is_active: true,
            staleness: StalenessLevel::Hot,
            daily_activity: Vec::new(),
        };

        upsert_project(&conn, &project).unwrap();

        let projects = get_all_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, "/Users/me/myproject");
        assert_eq!(projects[0].name, "myproject");
        assert_eq!(projects[0].total_sessions, 3);
        assert!(projects[0].is_active);
    }

    #[test]
    fn upsert_project_updates_on_conflict() {
        let conn = setup_db();

        let now = chrono::Utc::now().timestamp_millis();
        let project = Project {
            path: "/project".to_string(),
            name: "project".to_string(),
            first_seen: now,
            last_activity: now,
            total_sessions: 1,
            total_messages: 5,
            is_active: false,
            staleness: StalenessLevel::Warm,
            daily_activity: Vec::new(),
        };
        upsert_project(&conn, &project).unwrap();

        // Update with more sessions
        let updated = Project {
            total_sessions: 3,
            total_messages: 20,
            is_active: true,
            ..project
        };
        upsert_project(&conn, &updated).unwrap();

        let projects = get_all_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].total_sessions, 3);
        assert_eq!(projects[0].total_messages, 20);
        assert!(projects[0].is_active);
    }
}

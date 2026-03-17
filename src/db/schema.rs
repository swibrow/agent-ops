use anyhow::Result;
use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id      TEXT UNIQUE NOT NULL,
            pid             INTEGER,
            project_path    TEXT NOT NULL,
            project_name    TEXT NOT NULL,
            started_at      INTEGER NOT NULL,
            ended_at        INTEGER,
            status          TEXT NOT NULL DEFAULT 'unknown',
            first_prompt    TEXT,
            summary         TEXT,
            git_branch      TEXT,
            message_count   INTEGER DEFAULT 0,
            last_activity   INTEGER,
            tmux_session    TEXT,
            tmux_window     INTEGER,
            tmux_pane_id    TEXT,
            pane_title      TEXT
        );

        CREATE TABLE IF NOT EXISTS projects (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            path            TEXT UNIQUE NOT NULL,
            name            TEXT NOT NULL,
            first_seen      INTEGER NOT NULL,
            last_activity   INTEGER NOT NULL,
            total_sessions  INTEGER DEFAULT 0,
            total_messages  INTEGER DEFAULT 0,
            is_active       INTEGER DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS daily_activity (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            project_path    TEXT NOT NULL,
            date            TEXT NOT NULL,
            session_count   INTEGER DEFAULT 0,
            message_count   INTEGER DEFAULT 0,
            UNIQUE(project_path, date)
        );

        CREATE TABLE IF NOT EXISTS history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp       INTEGER NOT NULL,
            project_path    TEXT NOT NULL,
            display         TEXT NOT NULL,
            session_id      TEXT,
            UNIQUE(timestamp, project_path)
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_path);
        CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
        CREATE INDEX IF NOT EXISTS idx_history_project ON history(project_path);
        CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp);
        CREATE INDEX IF NOT EXISTS idx_daily_activity_project ON daily_activity(project_path);
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn run_migrations_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify tables exist by querying them
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_activity", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn run_migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // Running again should not error
        run_migrations(&conn).unwrap();
    }
}

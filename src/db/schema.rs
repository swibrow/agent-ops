use anyhow::Result;
use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id      TEXT UNIQUE NOT NULL,
            pid             INTEGER,
            agent_type      TEXT NOT NULL DEFAULT 'claude',
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

        CREATE TABLE IF NOT EXISTS messages (
            uuid            TEXT PRIMARY KEY,
            session_id      TEXT NOT NULL,
            agent_type      TEXT NOT NULL,
            role            TEXT NOT NULL,
            content         TEXT NOT NULL,
            timestamp       INTEGER NOT NULL,
            project_path    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ingest_state (
            file_path       TEXT PRIMARY KEY,
            agent_type      TEXT NOT NULL,
            mtime           INTEGER NOT NULL,
            size            INTEGER NOT NULL,
            message_count   INTEGER NOT NULL DEFAULT 0,
            last_ingested   INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_path);
        CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
        CREATE INDEX IF NOT EXISTS idx_history_project ON history(project_path);
        CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp);
        CREATE INDEX IF NOT EXISTS idx_daily_activity_project ON daily_activity(project_path);
        CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
        CREATE INDEX IF NOT EXISTS idx_messages_project ON messages(project_path);
        CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp);
        CREATE INDEX IF NOT EXISTS idx_messages_agent ON messages(agent_type);
        ",
    )?;

    // Migration: add agent_type column to existing databases that don't have it
    let has_agent_type: bool = conn
        .prepare("SELECT agent_type FROM sessions LIMIT 0")
        .is_ok();
    if !has_agent_type {
        conn.execute_batch(
            "ALTER TABLE sessions ADD COLUMN agent_type TEXT NOT NULL DEFAULT 'claude';",
        )?;
    }

    // Create the agent_type index (safe now that the column is guaranteed to exist)
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_sessions_agent_type ON sessions(agent_type);",
    )?;

    // Migration: add token usage columns to messages table
    let has_tokens: bool = conn
        .prepare("SELECT input_tokens FROM messages LIMIT 0")
        .is_ok();
    if !has_tokens {
        conn.execute_batch(
            "ALTER TABLE messages ADD COLUMN input_tokens INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE messages ADD COLUMN output_tokens INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE messages ADD COLUMN cache_creation_tokens INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE messages ADD COLUMN cache_read_tokens INTEGER NOT NULL DEFAULT 0;
             -- Clear ingest state so all files are re-ingested to backfill token data
             DELETE FROM ingest_state;",
        )?;
    }

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

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Verify agent_type column exists
        let _: String = conn
            .query_row("SELECT agent_type FROM sessions LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap_or_default();
    }

    #[test]
    fn run_migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn migration_adds_agent_type_to_existing_db() {
        let conn = Connection::open_in_memory().unwrap();
        // Create old schema without agent_type
        conn.execute_batch(
            "CREATE TABLE sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT UNIQUE NOT NULL,
                pid INTEGER,
                project_path TEXT NOT NULL,
                project_name TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                status TEXT NOT NULL DEFAULT 'unknown',
                first_prompt TEXT,
                summary TEXT,
                git_branch TEXT,
                message_count INTEGER DEFAULT 0,
                last_activity INTEGER,
                tmux_session TEXT,
                tmux_window INTEGER,
                tmux_pane_id TEXT,
                pane_title TEXT
            );",
        )
        .unwrap();

        // Insert a row without agent_type
        conn.execute(
            "INSERT INTO sessions (session_id, project_path, project_name, started_at)
             VALUES ('s1', '/p', 'p', 1000)",
            [],
        )
        .unwrap();

        // Run migrations — should add agent_type column
        run_migrations(&conn).unwrap();

        // Verify existing row defaults to 'claude'
        let agent_type: String = conn
            .query_row(
                "SELECT agent_type FROM sessions WHERE session_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(agent_type, "claude");
    }
}

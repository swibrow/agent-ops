use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::data::{claude, stats, tmux};
use crate::db::queries;
use crate::model::project::{Project, StalenessLevel};
use crate::model::session::{AgentActivity, AgentSession, SessionStatus};

/// Maps session_id -> activity for overlaying onto DB results
pub type ActivityMap = HashMap<String, AgentActivity>;

/// Raw data gathered from async I/O (tmux, ps, files).
/// This struct is Send and can be passed across threads.
pub struct GatheredData {
    pub agent_panes: Vec<tmux::AgentPaneInfo>,
    pub claude_sessions: Vec<claude::ClaudeSessionFile>,
    pub project_sessions: Vec<(String, Vec<claude::SessionIndexEntry>)>,
    pub process_tree: stats::ProcessTree,
    /// Activity refined by pane content sniffing
    pub pane_activities: HashMap<String, AgentActivity>,
}

/// Gather all data from async sources (tmux, filesystem, ps).
/// This is Send-safe — no DB references held across awaits.
pub async fn gather_data(config: &Config) -> Result<GatheredData> {
    // Step 1: Find all Claude agent panes (single tmux call)
    let agent_panes = tmux::find_agent_panes().await?;
    debug!(pane_count = agent_panes.len(), "found agent panes");

    // Step 2: Read Claude session files (sync I/O but fast)
    let claude_sessions = claude::read_active_sessions(&config.claude_dir).unwrap_or_default();

    // Step 3: Build process tree (single ps call)
    let process_tree = stats::ProcessTree::snapshot().await.unwrap_or_else(|e| {
        warn!(error = %e, "failed to snapshot process tree, using empty");
        stats::ProcessTree::empty()
    });

    // Step 4: Detect activity for each pane (may capture pane content)
    let mut pane_activities = HashMap::new();
    for pane in &agent_panes {
        let activity =
            tmux::detect_activity_from_content(&pane.tmux_ref.pane_id, pane.title_activity.clone())
                .await;
        pane_activities.insert(pane.tmux_ref.pane_id.clone(), activity);
    }

    // Step 5: Read project session indexes
    let project_sessions = claude::read_project_sessions(&config.claude_dir).unwrap_or_default();

    Ok(GatheredData {
        agent_panes,
        claude_sessions,
        project_sessions,
        process_tree,
        pane_activities,
    })
}

/// Apply gathered data to the database (sync, requires Connection).
/// Call this on the main thread after receiving GatheredData.
pub fn apply_to_db(conn: &Connection, data: &GatheredData) -> Result<SyncResult> {
    let mut result = SyncResult {
        active_pane_count: data.agent_panes.len(),
        ..Default::default()
    };

    // Build PID -> claude session map
    let mut pid_to_claude: HashMap<u32, &claude::ClaudeSessionFile> = HashMap::new();
    for cs in &data.claude_sessions {
        if let Some(pid) = cs.pid {
            pid_to_claude.insert(pid, cs);
        }
    }

    // Match agent panes to claude sessions and upsert
    let active_pane_ids: Vec<String> = data
        .agent_panes
        .iter()
        .map(|p| p.tmux_ref.pane_id.clone())
        .collect();
    let mut pane_pid_to_session_id: HashMap<u32, String> = HashMap::new();
    let mut claimed_session_ids: Vec<String> = Vec::new();

    for pane in &data.agent_panes {
        let activity = data
            .pane_activities
            .get(&pane.tmux_ref.pane_id)
            .cloned()
            .unwrap_or(pane.title_activity.clone());

        let claude_session = find_claude_session_for_pane(
            pane,
            &pid_to_claude,
            &claimed_session_ids,
            &data.process_tree,
        );

        let session = match claude_session {
            Some(cs) => AgentSession {
                session_id: cs.session_id.clone(),
                pid: cs.pid,
                project_path: cs.cwd.clone(),
                project_name: claude::project_name_from_path(&cs.cwd),
                started_at: cs.started_at,
                ended_at: None,
                status: SessionStatus::Active,
                first_prompt: None,
                summary: None,
                git_branch: None,
                message_count: 0,
                last_activity: Some(chrono::Utc::now().timestamp_millis()),
                tmux_pane: Some(pane.tmux_ref.clone()),
                pane_title: Some(pane.title.clone()),
                activity: activity.clone(),
                cpu_percent: 0.0,
                memory_mb: 0.0,
            },
            None => AgentSession {
                session_id: format!("tmux-{}", pane.tmux_ref.pane_id),
                pid: Some(pane.pid),
                project_path: pane.current_path.clone(),
                project_name: claude::project_name_from_path(&pane.current_path),
                started_at: chrono::Utc::now().timestamp_millis(),
                ended_at: None,
                status: SessionStatus::Active,
                first_prompt: None,
                summary: None,
                git_branch: None,
                message_count: 0,
                last_activity: Some(chrono::Utc::now().timestamp_millis()),
                tmux_pane: Some(pane.tmux_ref.clone()),
                pane_title: Some(pane.title.clone()),
                activity: activity.clone(),
                cpu_percent: 0.0,
                memory_mb: 0.0,
            },
        };

        claimed_session_ids.push(session.session_id.clone());
        pane_pid_to_session_id.insert(pane.pid, session.session_id.clone());
        result
            .activity_map
            .insert(session.session_id.clone(), activity);

        queries::upsert_session(conn, &session)?;
        result.sessions_synced += 1;
    }

    // Mark sessions that no longer have tmux panes as completed
    let marked = queries::mark_sessions_completed(conn, &active_pane_ids)?;
    result.sessions_completed = marked;

    // Import project session history
    for (project_path, entries) in &data.project_sessions {
        for entry in entries {
            let session = AgentSession {
                session_id: entry.session_id.clone(),
                pid: None,
                project_path: project_path.clone(),
                project_name: claude::project_name_from_path(project_path),
                started_at: entry.created.unwrap_or(0),
                ended_at: entry.modified,
                status: SessionStatus::Completed,
                first_prompt: entry.first_prompt.clone(),
                summary: entry.summary.clone(),
                git_branch: entry.git_branch.clone(),
                message_count: entry.message_count.unwrap_or(0),
                last_activity: entry.modified.or(entry.created),
                tmux_pane: None,
                pane_title: None,
                activity: AgentActivity::Unknown,
                cpu_percent: 0.0,
                memory_mb: 0.0,
            };
            queries::upsert_session(conn, &session)?;
        }
    }

    // Update project aggregates
    sync_project_aggregates(conn)?;

    // Collect CPU/RAM stats using the process tree
    let mut all_pids: Vec<u32> = data.agent_panes.iter().map(|p| p.pid).collect();
    let self_pid = std::process::id();
    all_pids.push(self_pid);

    let stats_by_pid = data.process_tree.collect_stats(&all_pids);

    for (pane_pid, session_id) in &pane_pid_to_session_id {
        if let Some(ps) = stats_by_pid.get(pane_pid) {
            result.stats_map.insert(session_id.clone(), ps.clone());
        }
    }

    result.self_stats = stats_by_pid.get(&self_pid).cloned().unwrap_or_default();

    info!(
        synced = result.sessions_synced,
        completed = result.sessions_completed,
        panes = result.active_pane_count,
        "sync complete"
    );

    Ok(result)
}

/// Full sync: gather data + apply to DB. Convenience for initial sync on main thread.
pub async fn full_sync(config: &Config, conn: &Connection) -> Result<SyncResult> {
    let data = gather_data(config).await?;
    apply_to_db(conn, &data)
}

/// Import history.jsonl entries into DB.
pub fn import_history(config: &Config, conn: &Connection) -> Result<usize> {
    let entries = claude::read_history(&config.claude_dir)?;
    let mut imported = 0;
    for entry in &entries {
        if queries::insert_history(conn, entry).is_ok() {
            imported += 1;
        }
    }

    if imported > 0 {
        queries::rebuild_daily_activity(conn)?;
        info!(count = imported, "imported history entries");
    }

    Ok(imported)
}

fn sync_project_aggregates(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT project_path, project_name,
                MIN(started_at) as first_seen,
                MAX(COALESCE(last_activity, started_at)) as last_activity,
                COUNT(*) as total_sessions,
                SUM(message_count) as total_messages,
                SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END) as active_count
         FROM sessions
         GROUP BY project_path",
    )?;

    let now = chrono::Utc::now().timestamp_millis();

    let projects: Vec<Project> = stmt
        .query_map([], |row| {
            let last_activity: i64 = row.get(3)?;
            let hours_since = (now - last_activity) / (1000 * 3600);
            let active_count: i32 = row.get(6)?;
            let is_active = active_count > 0;

            Ok(Project {
                path: row.get(0)?,
                name: row.get(1)?,
                first_seen: row.get(2)?,
                last_activity,
                total_sessions: row.get(4)?,
                total_messages: row.get::<_, Option<u32>>(5)?.unwrap_or(0),
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

    for project in &projects {
        queries::upsert_project(conn, project)?;
    }

    Ok(())
}

fn find_claude_session_for_pane<'a>(
    pane: &tmux::AgentPaneInfo,
    pid_map: &'a HashMap<u32, &'a claude::ClaudeSessionFile>,
    claimed_session_ids: &[String],
    tree: &stats::ProcessTree,
) -> Option<&'a claude::ClaudeSessionFile> {
    // Direct PID match
    if let Some(session) = pid_map.get(&pane.pid) {
        if !claimed_session_ids.contains(&session.session_id) {
            return Some(session);
        }
    }

    // Check if any claude session PID is a descendant of the pane PID
    for (pid, session) in pid_map {
        if claimed_session_ids.contains(&session.session_id) {
            continue;
        }
        if tree.is_descendant_of(pane.pid, *pid) {
            return Some(session);
        }
    }

    // Fallback: match by cwd, only if exactly one unclaimed session matches
    let cwd_matches: Vec<_> = pid_map
        .values()
        .filter(|s| s.cwd == pane.current_path && !claimed_session_ids.contains(&s.session_id))
        .collect();
    if cwd_matches.len() == 1 {
        return Some(cwd_matches[0]);
    }

    None
}

#[derive(Debug, Default)]
pub struct SyncResult {
    pub active_pane_count: usize,
    pub sessions_synced: usize,
    pub sessions_completed: usize,
    pub activity_map: ActivityMap,
    pub stats_map: HashMap<String, stats::ProcessStats>,
    pub self_stats: stats::ProcessStats,
}

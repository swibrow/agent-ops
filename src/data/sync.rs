use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;
use tracing::{debug, info, warn};

use crate::agent::{AgentRegistry, AgentSessionFile};
use crate::config::Config;
use crate::data::{claude, stats, tmux};
use crate::db::queries;
use crate::model::project::{Project, StalenessLevel};
use crate::model::session::{AgentActivity, AgentSession, AgentType, SessionStatus};
use crate::model::tmux::TmuxPaneRef;

/// Maps session_id -> activity for overlaying onto DB results
pub type ActivityMap = HashMap<String, AgentActivity>;

/// Raw data gathered from async I/O (tmux, ps, files).
/// This struct is Send and can be passed across threads.
pub struct GatheredData {
    pub agent_panes: Vec<tmux::AgentPaneInfo>,
    pub session_files: Vec<AgentSessionFile>,
    pub project_sessions: Vec<(String, Vec<claude::SessionIndexEntry>)>,
    pub process_tree: stats::ProcessTree,
    pub pane_activities: HashMap<String, AgentActivity>,
}

/// Gather all data from async sources (tmux, filesystem, ps).
/// Uses the registry to detect agents across all providers.
pub async fn gather_data(config: &Config, registry: &AgentRegistry) -> Result<GatheredData> {
    // Step 1: List all tmux panes (single call)
    let all_panes = tmux::list_all_panes().await?;

    // Step 2: Detect agents using the registry
    let mut agent_panes = Vec::new();
    for (session_name, window_index, pane) in &all_panes {
        if let Some(provider) = registry.detect(pane) {
            let activity = provider.detect_activity(pane);
            let title = provider.clean_title(&pane.title).to_string();
            agent_panes.push(tmux::AgentPaneInfo {
                tmux_ref: TmuxPaneRef {
                    session_name: session_name.clone(),
                    window_index: *window_index,
                    pane_id: pane.id.clone(),
                },
                pid: pane.pid,
                title,
                current_path: pane.current_path.clone(),
                agent_type: provider.agent_type(),
                title_activity: activity,
            });
        }
    }
    debug!(pane_count = agent_panes.len(), "found agent panes");

    // Step 3: Build process tree (single ps call)
    let process_tree = stats::ProcessTree::snapshot().await.unwrap_or_else(|e| {
        warn!(error = %e, "failed to snapshot process tree, using empty");
        stats::ProcessTree::empty()
    });

    // Step 4: Gather session files from all providers
    let mut session_files = Vec::new();
    for provider in registry.providers() {
        match provider.gather_sessions(config).await {
            Ok(sessions) => session_files.extend(sessions),
            Err(e) => {
                warn!(
                    agent = provider.agent_type().as_str(),
                    error = %e,
                    "failed to gather sessions"
                );
            }
        }
    }

    // Step 5: Refine activity for each pane (provider-specific I/O)
    let mut pane_activities = HashMap::new();
    for pane in &agent_panes {
        // Find the provider for this pane's agent type
        let refined = if let Some(provider) = registry
            .providers()
            .iter()
            .find(|p| p.agent_type() == pane.agent_type)
        {
            provider
                .refine_activity(&pane.tmux_ref.pane_id, pane.title_activity.clone())
                .await
        } else {
            pane.title_activity.clone()
        };
        pane_activities.insert(pane.tmux_ref.pane_id.clone(), refined);
    }

    // Step 6: Read project session indexes (Claude-specific, but still useful for history)
    let project_sessions = claude::read_project_sessions(&config.claude_dir).unwrap_or_default();

    Ok(GatheredData {
        agent_panes,
        session_files,
        project_sessions,
        process_tree,
        pane_activities,
    })
}

/// Apply gathered data to the database (sync, requires Connection).
pub fn apply_to_db(conn: &Connection, data: &GatheredData) -> Result<SyncResult> {
    let mut result = SyncResult {
        active_pane_count: data.agent_panes.len(),
        ..Default::default()
    };

    // Build PID -> session file map
    let mut pid_to_session: HashMap<u32, &AgentSessionFile> = HashMap::new();
    for sf in &data.session_files {
        if let Some(pid) = sf.pid {
            pid_to_session.insert(pid, sf);
        }
    }

    // Match agent panes to session files and upsert
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

        let matched_session = find_session_for_pane(
            pane,
            &pid_to_session,
            &claimed_session_ids,
            &data.process_tree,
        );

        let session = build_session(pane, matched_session, &activity);

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

    // Import project session history (Claude-specific)
    for (project_path, entries) in &data.project_sessions {
        for entry in entries {
            let session = AgentSession {
                session_id: entry.session_id.clone(),
                pid: None,
                agent_type: AgentType::Claude,
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

    // Collect CPU/RAM stats
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

/// Full sync: gather data + apply to DB.
pub async fn full_sync(
    config: &Config,
    conn: &Connection,
    registry: &AgentRegistry,
) -> Result<SyncResult> {
    let data = gather_data(config, registry).await?;
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

fn build_session(
    pane: &tmux::AgentPaneInfo,
    matched: Option<&AgentSessionFile>,
    activity: &AgentActivity,
) -> AgentSession {
    // Only update last_activity when the agent is actively doing work.
    // For idle/waiting states, pass None so the DB preserves the previous value.
    let last_activity = match activity {
        AgentActivity::Processing => Some(chrono::Utc::now().timestamp_millis()),
        _ => None,
    };

    match matched {
        Some(sf) => AgentSession {
            session_id: sf.session_id.clone(),
            pid: sf.pid,
            agent_type: pane.agent_type,
            project_path: sf.cwd.clone(),
            project_name: claude::project_name_from_path(&sf.cwd),
            started_at: sf.started_at,
            ended_at: None,
            status: SessionStatus::Active,
            first_prompt: None,
            summary: None,
            git_branch: None,
            message_count: 0,
            last_activity,
            tmux_pane: Some(pane.tmux_ref.clone()),
            pane_title: Some(pane.title.clone()),
            activity: activity.clone(),
            cpu_percent: 0.0,
            memory_mb: 0.0,
        },
        None => AgentSession {
            session_id: format!("tmux-{}", pane.tmux_ref.pane_id),
            pid: Some(pane.pid),
            agent_type: pane.agent_type,
            project_path: pane.current_path.clone(),
            project_name: claude::project_name_from_path(&pane.current_path),
            started_at: chrono::Utc::now().timestamp_millis(),
            ended_at: None,
            status: SessionStatus::Active,
            first_prompt: None,
            summary: None,
            git_branch: None,
            message_count: 0,
            last_activity,
            tmux_pane: Some(pane.tmux_ref.clone()),
            pane_title: Some(pane.title.clone()),
            activity: activity.clone(),
            cpu_percent: 0.0,
            memory_mb: 0.0,
        },
    }
}

fn find_session_for_pane<'a>(
    pane: &tmux::AgentPaneInfo,
    pid_map: &'a HashMap<u32, &'a AgentSessionFile>,
    claimed_session_ids: &[String],
    tree: &stats::ProcessTree,
) -> Option<&'a AgentSessionFile> {
    if let Some(session) = pid_map.get(&pane.pid) {
        if !claimed_session_ids.contains(&session.session_id) {
            return Some(session);
        }
    }

    for (pid, session) in pid_map {
        if claimed_session_ids.contains(&session.session_id) {
            continue;
        }
        if tree.is_descendant_of(pane.pid, *pid) {
            return Some(session);
        }
    }

    let cwd_matches: Vec<_> = pid_map
        .values()
        .filter(|s| s.cwd == pane.current_path && !claimed_session_ids.contains(&s.session_id))
        .collect();
    if cwd_matches.len() == 1 {
        return Some(cwd_matches[0]);
    }

    None
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

#[derive(Debug, Default)]
pub struct SyncResult {
    pub active_pane_count: usize,
    pub sessions_synced: usize,
    pub sessions_completed: usize,
    pub activity_map: ActivityMap,
    pub stats_map: HashMap<String, stats::ProcessStats>,
    pub self_stats: stats::ProcessStats,
}

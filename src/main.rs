mod agent;
mod app;
mod config;
mod data;
mod db;
mod event;
mod model;
mod tui;
mod ui;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, KeyEventKind};
use rusqlite::Connection;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::agent::AgentRegistry;
use crate::app::App;
use crate::config::Config;
use crate::data::notify::NotificationTracker;
use crate::data::sync;
use crate::db::schema;
use crate::event::action::Action;
use crate::event::handler::handle_key_event;
use crate::model::session::AgentActivity;

/// agent-ops — htop for your AI coding agents
///
/// Monitor, track, and resume Claude Code and other AI agent sessions
/// running in tmux.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Polling interval in seconds for background sync
    #[arg(short, long, default_value_t = 3)]
    poll_interval: u64,

    /// Disable desktop notifications
    #[arg(long)]
    no_notifications: bool,

    /// Reset the database and re-import all session data
    #[arg(long)]
    reset_db: bool,

    /// Print the log file path and exit
    #[arg(long)]
    log_path: bool,

    /// Print the database path and exit
    #[arg(long)]
    db_path: bool,

    /// Additional Claude config directories to monitor (repeatable)
    #[arg(long = "claude-dir", value_name = "DIR")]
    claude_dirs: Vec<PathBuf>,

    /// Print the config file path and exit
    #[arg(long)]
    config_path: bool,
}

/// Messages sent from background tasks to the UI
enum SyncMsg {
    Data(sync::GatheredData),
    Error(String),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::new(&cli.claude_dirs)?;

    // Apply CLI overrides
    config.poll_interval_secs = cli.poll_interval;
    if cli.no_notifications {
        config.notifications_enabled = false;
    }

    // Info-only flags that print and exit
    if cli.log_path {
        println!("{}", config.log_path.display());
        return Ok(());
    }
    if cli.db_path {
        println!("{}", config.db_path.display());
        return Ok(());
    }
    if cli.config_path {
        println!("{}", Config::config_path().display());
        return Ok(());
    }

    if let Some(parent) = config.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Reset DB if requested
    if cli.reset_db && config.db_path.exists() {
        std::fs::remove_file(&config.db_path)?;
        eprintln!("Database reset: {}", config.db_path.display());
    }

    // Set up logging
    let log_dir = config.log_path.parent().unwrap().to_path_buf();
    let log_filename = config
        .log_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let file_appender = tracing_appender::rolling::never(&log_dir, &log_filename);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .init();

    info!("agent-ops starting");

    // Agent registry
    let registry = Arc::new(AgentRegistry::default_registry());

    // Open DB and run migrations
    let conn = Connection::open(&config.db_path)?;
    schema::run_migrations(&conn)?;

    // Import history on first run
    let history_count = sync::import_history(&config, &conn)?;
    if history_count > 0 {
        info!(count = history_count, "imported history entries");
    }

    // Initial sync
    let sync_result = sync::full_sync(&config, &conn, &registry).await?;

    // Initialize app state
    let mut app = App::new();
    app.apply_sync_result(&sync_result);
    app.refresh_from_db(&conn);

    // Set up background sync channel
    let (sync_tx, mut sync_rx) = mpsc::unbounded_channel::<SyncMsg>();

    // Spawn background sync task
    let sync_claude_dirs = config.claude_dirs.clone();
    let sync_poll_interval = Duration::from_secs(config.poll_interval_secs);
    let sync_tx_poll = sync_tx.clone();
    let sync_registry = Arc::clone(&registry);
    tokio::spawn(async move {
        let cfg = Config {
            claude_dirs: sync_claude_dirs,
            db_path: PathBuf::new(),
            log_path: PathBuf::new(),
            poll_interval_secs: 0,
            tick_rate_ms: 0,
            notifications_enabled: false,
        };

        loop {
            tokio::time::sleep(sync_poll_interval).await;

            match sync::gather_data(&cfg, &sync_registry).await {
                Ok(data) => {
                    let _ = sync_tx_poll.send(SyncMsg::Data(data));
                }
                Err(e) => {
                    let _ = sync_tx_poll.send(SyncMsg::Error(format!("Sync failed: {e}")));
                }
            }
        }
    });

    // Spawn filesystem watchers (one per claude dir)
    for claude_dir in &config.claude_dirs {
        let watch_dir = claude_dir.join("sessions");
        if watch_dir.exists() {
            let watch_tx = sync_tx.clone();
            let watch_claude_dirs = config.claude_dirs.clone();
            let watch_registry = Arc::clone(&registry);
            tokio::spawn(async move {
                if let Err(e) =
                    run_file_watcher(watch_dir, watch_tx, watch_claude_dirs, watch_registry).await
                {
                    warn!(error = %e, "filesystem watcher failed");
                }
            });
        }
    }

    // Notification tracker
    let mut notifier = NotificationTracker::new(config.notifications_enabled);

    // Track which tmux windows we've renamed so we can reset them on shutdown
    let mut renamed_windows: HashSet<String> = HashSet::new();

    // Track per-window activity from last sync cycle to detect transitions.
    let mut prev_window_activities: HashMap<String, AgentActivity> = HashMap::new();
    // Windows showing ✅ (completed) and when they completed.
    let mut completed_windows: HashMap<String, Instant> = HashMap::new();
    let completed_expiry = Duration::from_secs(600); // 10 minutes

    // Initialize terminal
    let mut terminal = tui::init()?;

    let tick_rate = Duration::from_millis(config.tick_rate_ms);

    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        while let Ok(msg) = sync_rx.try_recv() {
            match msg {
                SyncMsg::Data(data) => match sync::apply_to_db(&conn, &data) {
                    Ok(result) => {
                        let session_info: HashMap<String, data::notify::SessionInfo> = app
                            .active_sessions
                            .iter()
                            .map(|s| {
                                (
                                    s.session_id.clone(),
                                    data::notify::SessionInfo {
                                        agent_type: s.agent_type,
                                        project_name: s.project_name.clone(),
                                    },
                                )
                            })
                            .collect();
                        notifier.check_transitions(
                            &result.activity_map,
                            &session_info,
                            &result.active_session_ids,
                        );

                        // Build per-window activity states from gathered pane data.
                        // Multiple panes can share a window — pick the highest priority.
                        let mut window_activities: HashMap<String, AgentActivity> =
                            HashMap::new();
                        for pane in &data.agent_panes {
                            let target = format!(
                                "{}:{}",
                                pane.tmux_ref.session_name, pane.tmux_ref.window_index
                            );
                            let activity = data
                                .pane_activities
                                .get(&pane.tmux_ref.pane_id)
                                .cloned()
                                .unwrap_or(pane.title_activity.clone());
                            let entry = window_activities.entry(target).or_default();
                            if activity.sort_priority() < entry.sort_priority() {
                                *entry = activity;
                            }
                        }

                        let now = Instant::now();

                        // Detect completion transitions:
                        // 1. Processing → WaitingForInput (agent finished work)
                        // 2. Previously active window disappeared (agent exited)
                        for (target, prev_activity) in &prev_window_activities {
                            let cur_activity = window_activities.get(target);
                            let just_completed = match (prev_activity, cur_activity) {
                                // Agent was working, now idle → done
                                (
                                    AgentActivity::Processing,
                                    Some(AgentActivity::WaitingForInput),
                                ) => true,
                                // Agent pane disappeared entirely → exited
                                (AgentActivity::Processing, None) => true,
                                (AgentActivity::WaitingForInput, None) => true,
                                (AgentActivity::WaitingForPermission, None) => true,
                                (AgentActivity::Unknown, None) => true,
                                _ => false,
                            };
                            if just_completed && !completed_windows.contains_key(target) {
                                completed_windows.insert(target.clone(), now);
                            }
                        }

                        // If agent starts processing again, clear completed state
                        for (target, activity) in &window_activities {
                            if matches!(activity, AgentActivity::Processing) {
                                completed_windows.remove(target);
                            }
                        }

                        // Expire completed windows older than 10 minutes
                        let expired: Vec<String> = completed_windows
                            .iter()
                            .filter(|(_, ts)| now.duration_since(**ts) >= completed_expiry)
                            .map(|(t, _)| t.clone())
                            .collect();
                        if !expired.is_empty() {
                            for t in &expired {
                                completed_windows.remove(t);
                            }
                            // Reset icons for windows where agent exited (no longer in panes)
                            let exited: Vec<String> = expired
                                .iter()
                                .filter(|t| !window_activities.contains_key(*t))
                                .cloned()
                                .collect();
                            if !exited.is_empty() {
                                if let Err(e) =
                                    data::tmux::reset_agent_window_titles(&exited).await
                                {
                                    warn!(
                                        error = %e,
                                        "failed to reset expired completed windows"
                                    );
                                }
                                for t in &exited {
                                    renamed_windows.remove(t);
                                }
                            }
                        }

                        // Override activity to Completed for windows in the completed set
                        for (target, _) in &completed_windows {
                            window_activities
                                .insert(target.clone(), AgentActivity::Completed);
                        }

                        // Save current activities for next cycle's transition detection
                        prev_window_activities = window_activities
                            .iter()
                            .filter(|(_, a)| !matches!(a, AgentActivity::Completed))
                            .map(|(t, a): (&String, &AgentActivity)| (t.clone(), a.clone()))
                            .collect();
                        // Also remember windows that disappeared so we can detect exits
                        // (prev_window_activities already excludes completed overrides)

                        let window_states: Vec<data::tmux::AgentWindowState> = window_activities
                            .into_iter()
                            .map(|(target, activity)| data::tmux::AgentWindowState {
                                target,
                                activity,
                            })
                            .collect();

                        // Remember which windows we've touched
                        for ws in &window_states {
                            renamed_windows.insert(ws.target.clone());
                        }

                        if let Err(e) = data::tmux::update_agent_window_titles(&window_states).await
                        {
                            warn!(error = %e, "failed to update tmux window titles");
                        }

                        app.apply_sync_result(&result);
                        app.refresh_from_db(&conn);
                    }
                    Err(e) => {
                        error!(error = %e, "failed to apply sync data to DB");
                        app.last_error = Some(format!("DB write failed: {e}"));
                    }
                },
                SyncMsg::Error(e) => {
                    error!(error = e, "background sync error");
                    app.last_error = Some(e);
                }
            }
        }

        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = crossterm::event::read()? {
                if key.kind == KeyEventKind::Press {
                    let action = handle_key_event(key, &app);
                    app.apply_action(action);
                }
            }
        } else {
            app.apply_action(Action::Tick);
        }

        if app.show_detail && app.pane_preview.is_none() {
            if let Some(session) = app.selected_session() {
                if let Some(ref pane) = session.tmux_pane {
                    let pane_id = pane.pane_id.clone();
                    if let Ok(preview) = data::tmux::capture_pane(&pane_id, 10).await {
                        app.pane_preview = Some(preview);
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    tui::restore()?;

    // Reset all agent tmux windows back to automatic naming
    let targets: Vec<String> = renamed_windows.into_iter().collect();
    if let Err(e) = data::tmux::reset_agent_window_titles(&targets).await {
        warn!(error = %e, "failed to reset tmux window titles on shutdown");
    }

    info!("agent-ops shutdown");

    Ok(())
}

async fn run_file_watcher(
    dir: PathBuf,
    tx: mpsc::UnboundedSender<SyncMsg>,
    claude_dirs: Vec<PathBuf>,
    registry: Arc<AgentRegistry>,
) -> Result<()> {
    use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};

    let (notify_tx, mut notify_rx) = mpsc::unbounded_channel();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = notify_tx.send(event);
            }
        },
        NotifyConfig::default(),
    )?;

    watcher.watch(&dir, RecursiveMode::NonRecursive)?;
    info!(dir = %dir.display(), "watching for session file changes");

    let debounce = Duration::from_millis(500);
    let mut last_sync = std::time::Instant::now();

    let cfg = Config {
        claude_dirs,
        db_path: PathBuf::new(),
        log_path: PathBuf::new(),
        poll_interval_secs: 0,
        tick_rate_ms: 0,
        notifications_enabled: false,
    };

    loop {
        if notify_rx.recv().await.is_none() {
            break;
        }

        while notify_rx.try_recv().is_ok() {}

        if last_sync.elapsed() < debounce {
            continue;
        }

        match sync::gather_data(&cfg, &registry).await {
            Ok(data) => {
                let _ = tx.send(SyncMsg::Data(data));
            }
            Err(e) => {
                let _ = tx.send(SyncMsg::Error(format!("Watch-triggered sync failed: {e}")));
            }
        }
        last_sync = std::time::Instant::now();
    }

    Ok(())
}

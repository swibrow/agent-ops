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
use std::time::Duration;

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
}

/// Messages sent from background tasks to the UI
enum SyncMsg {
    Data(sync::GatheredData),
    Error(String),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::new()?;

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
    let sync_claude_dir = config.claude_dir.clone();
    let sync_poll_interval = Duration::from_secs(config.poll_interval_secs);
    let sync_tx_poll = sync_tx.clone();
    let sync_registry = Arc::clone(&registry);
    tokio::spawn(async move {
        let cfg = Config {
            claude_dir: sync_claude_dir,
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

    // Spawn filesystem watcher
    let watch_dir = config.claude_dir.join("sessions");
    let watch_tx = sync_tx.clone();
    if watch_dir.exists() {
        let watch_claude_dir = config.claude_dir.clone();
        let watch_registry = Arc::clone(&registry);
        tokio::spawn(async move {
            if let Err(e) =
                run_file_watcher(watch_dir, watch_tx, watch_claude_dir, watch_registry).await
            {
                warn!(error = %e, "filesystem watcher failed");
            }
        });
    }

    // Notification tracker
    let mut notifier = NotificationTracker::new(config.notifications_enabled);

    // Track which tmux windows we've renamed so we can reset them on shutdown
    let mut renamed_windows: HashSet<String> = HashSet::new();

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
                        notifier.check_transitions(&result.activity_map, &session_info);

                        // Build per-window activity states from gathered pane data.
                        // Multiple panes can share a window — pick the highest priority.
                        let mut window_activities: HashMap<String, model::session::AgentActivity> =
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
    claude_dir: PathBuf,
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
        claude_dir,
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

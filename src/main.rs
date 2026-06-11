mod agent;
mod app;
mod config;
mod data;
mod db;
mod event;
mod model;
mod tui;
mod ui;
mod web;

use std::collections::HashMap;
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

    /// Start the web server alongside the TUI.
    /// Use --web-only to run the web server without the TUI.
    #[arg(long)]
    web: bool,

    /// Start the web server without the TUI (implies --web).
    #[arg(long)]
    web_only: bool,

    /// Port for the web server (used with --web / --web-only)
    #[arg(long, default_value_t = 3000)]
    port: u16,
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

    // --web-only: headless web server, no TUI.
    if cli.web_only {
        return web::run(&config, registry, cli.port).await;
    }

    // --web: spin up the web server alongside the TUI so you can browse
    // from the dashboard while working. We spawn it as a background task
    // and continue into the TUI path below.
    if cli.web {
        let web_config = crate::config::Config {
            claude_dirs: config.claude_dirs.clone(),
            db_path: config.db_path.clone(),
            log_path: config.log_path.clone(),
            poll_interval_secs: config.poll_interval_secs,
            tick_rate_ms: config.tick_rate_ms,
            notifications_enabled: false, // TUI handles notifications
            transcript_retention_days: config.transcript_retention_days,
        };
        let web_registry = Arc::clone(&registry);
        let port = cli.port;
        tokio::spawn(async move {
            if let Err(e) = web::run_embedded(&web_config, web_registry, port).await {
                warn!(error = %e, "embedded web server exited");
            }
        });
    }

    // Open DB and run migrations
    let conn = Connection::open(&config.db_path)?;
    schema::run_migrations(&conn)?;

    // Import history on first run
    let history_count = sync::import_history(&config, &conn)?;
    if history_count > 0 {
        info!(count = history_count, "imported history entries");
    }

    // Ingest full transcripts (idempotent; cheap on repeat)
    if let Err(e) = sync::ingest_transcripts(&config, &conn, &registry) {
        warn!(error = %e, "initial transcript ingest failed");
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
    let sync_db_path = config.db_path.clone();
    let sync_retention = config.transcript_retention_days;
    tokio::spawn(async move {
        let cfg = Config {
            claude_dirs: sync_claude_dirs,
            db_path: sync_db_path,
            log_path: PathBuf::new(),
            poll_interval_secs: 0,
            tick_rate_ms: 0,
            notifications_enabled: false,
            transcript_retention_days: sync_retention,
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

            // Hot-reload transcripts: re-scan JSONL files for changes.
            // Uses its own short-lived connection to avoid contention with the
            // main thread's DB usage.
            if let Ok(ingest_conn) = rusqlite::Connection::open(&cfg.db_path) {
                if let Err(e) = sync::ingest_transcripts(&cfg, &ingest_conn, &sync_registry) {
                    warn!(error = %e, "background transcript ingest failed");
                }
            }
        }
    });

    // Spawn filesystem watchers. We watch both `sessions/` (active session
    // metadata) and `projects/` (full JSONL transcripts, recursively) so that
    // transcript ingest hot-reloads when any agent writes.
    {
        let mut watch_dirs: Vec<PathBuf> = Vec::new();
        for claude_dir in &config.claude_dirs {
            let sessions_dir = claude_dir.join("sessions");
            if sessions_dir.exists() {
                watch_dirs.push(sessions_dir);
            }
            let projects_dir = claude_dir.join("projects");
            if projects_dir.exists() {
                watch_dirs.push(projects_dir);
            }
        }
        if !watch_dirs.is_empty() {
            let watch_tx = sync_tx.clone();
            let watch_claude_dirs = config.claude_dirs.clone();
            let watch_registry = Arc::clone(&registry);
            let watch_db_path = config.db_path.clone();
            let watch_retention = config.transcript_retention_days;
            tokio::spawn(async move {
                if let Err(e) = run_file_watcher(
                    watch_dirs,
                    watch_tx,
                    watch_claude_dirs,
                    watch_registry,
                    watch_db_path,
                    watch_retention,
                )
                .await
                {
                    warn!(error = %e, "filesystem watcher failed");
                }
            });
        }
    }

    // Notification tracker
    let mut notifier = NotificationTracker::new(config.notifications_enabled);

    // Tmux window title icons (🔔 💬 ⏳ …) — owns all cross-cycle rename state.
    let mut title_manager = data::window_titles::TitleManager::new();

    // Initialize terminal
    let mut terminal = tui::init()?;

    let tick_rate = Duration::from_millis(config.tick_rate_ms);

    loop {
        // Refresh review data lazily when it's dirty and visible.
        if app.review_dirty && matches!(app.active_view, crate::app::ActiveView::Review) {
            app.load_review(&conn);
        }

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

                        if let Err(e) = title_manager.apply(&data).await {
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

        // Execute side effects queued by apply_action (pane actions, resume,
        // clipboard, editor) — key handling itself never does I/O.
        let commands: Vec<app::AppCommand> = app.pending_commands.drain(..).collect();
        for command in commands {
            execute_app_command(&mut app, &registry, command).await;
        }

        // Load the pane capture for the detail / preview overlays. The live
        // preview refreshes every ~10 ticks (500ms) so it follows the agent.
        if app.show_detail || app.show_preview {
            let stale = app.pane_preview.is_none()
                || (app.show_preview && app.tick_count.is_multiple_of(10));
            if stale {
                if let Some(session) = app.selected_session() {
                    if let Some(ref pane) = session.tmux_pane {
                        let pane_id = pane.pane_id.clone();
                        if let Ok(preview) =
                            data::tmux::capture_pane_colored(&pane_id, 100).await
                        {
                            app.pane_preview = Some(preview);
                        }
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    tui::restore()?;

    // Restore all agent tmux window titles (and their automatic-rename setting)
    if let Err(e) = title_manager.reset_all().await {
        warn!(error = %e, "failed to reset tmux window titles on shutdown");
    }

    info!("agent-ops shutdown");

    Ok(())
}

/// Execute a side effect queued by `apply_action`.
async fn execute_app_command(app: &mut App, registry: &AgentRegistry, command: app::AppCommand) {
    use crate::app::AppCommand;

    match command {
        AppCommand::Pane(req) => execute_pane_request(app, registry, req).await,
        AppCommand::Resume(session) => execute_resume(app, registry, *session).await,
        AppCommand::CopyToClipboard(text) => execute_copy(app, text).await,
        AppCommand::OpenEditor { name, path } => execute_open_editor(app, name, path),
    }
}

/// Jump to a session's live tmux pane, or resume a detached session in a new
/// window. Pane-id targets are stable — tmux resolves the containing window
/// and session from them, so this survives window moves and renumbering.
async fn execute_resume(app: &mut App, registry: &AgentRegistry, session: model::session::AgentSession) {
    let result = if let Some(pane) = &session.tmux_pane {
        let mut commands: Vec<Vec<String>> = vec![
            vec![
                "select-pane".to_string(),
                "-t".to_string(),
                pane.pane_id.clone(),
            ],
            vec![
                "select-window".to_string(),
                "-t".to_string(),
                pane.pane_id.clone(),
            ],
        ];
        // Only a client running inside tmux can be switched; outside, the
        // select calls still mark the window/pane current for the next attach.
        if std::env::var_os("TMUX").is_some() {
            commands.push(vec![
                "switch-client".to_string(),
                "-t".to_string(),
                pane.pane_id.clone(),
            ]);
        }
        data::tmux::run_chained(&commands)
            .await
            .map(|()| format!("Jumped to {}", session.project_name))
    } else {
        let resume_cmd = registry
            .provider_for(session.agent_type)
            .and_then(|p| p.resume_command(&session.session_id));
        match resume_cmd {
            Some(resume_cmd) => {
                let mut cmd = vec![
                    "new-window".to_string(),
                    "-n".to_string(),
                    session.project_name.clone(),
                    "-c".to_string(),
                    session.project_path.clone(),
                ];
                cmd.extend(resume_cmd);
                data::tmux::run_chained(&[cmd])
                    .await
                    .map(|()| format!("Resumed: {}", session.project_name))
            }
            None => {
                app.status_message = Some(format!(
                    "Resume not supported for {}",
                    session.agent_type.label()
                ));
                return;
            }
        }
    };

    match result {
        Ok(message) => {
            info!(session_id = session.session_id, "resumed session");
            app.status_message = Some(message);
        }
        Err(e) => {
            warn!(error = %e, session_id = session.session_id, "resume failed");
            app.last_error = Some(format!("Resume failed: {e}"));
        }
    }
}

async fn execute_copy(app: &mut App, text: String) {
    use tokio::io::AsyncWriteExt;

    let result = async {
        let mut child = tokio::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).await?;
        }
        child.wait().await?;
        anyhow::Ok(())
    }
    .await;

    match result {
        Ok(()) => app.status_message = Some(format!("Copied: {text}")),
        Err(e) => {
            warn!(error = %e, "failed to copy to clipboard");
            app.last_error = Some("Failed to copy to clipboard".to_string());
        }
    }
}

fn execute_open_editor(app: &mut App, name: String, path: String) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
    match std::process::Command::new(&editor).arg(&path).spawn() {
        Ok(_) => {
            app.status_message = Some(format!("Opened {name} in {editor}"));
        }
        Err(e) => {
            warn!(error = %e, editor, "failed to open editor");
            app.last_error = Some(format!("Failed to open {editor}: {e}"));
        }
    }
}

/// Deliver a queued pane action via tmux send-keys and reflect the outcome
/// in the UI immediately, ahead of the next sync cycle.
async fn execute_pane_request(app: &mut App, registry: &AgentRegistry, req: app::PaneRequest) {
    use crate::app::PaneCommand;

    let provider = registry.provider_for(req.agent_type);
    let result = match &req.command {
        PaneCommand::Approve => {
            let keys = provider
                .map(|p| p.approve_keys())
                .unwrap_or_else(|| vec!["1".to_string()]);
            data::tmux::send_keys(&req.pane_id, &keys).await
        }
        PaneCommand::Deny => {
            let keys = provider
                .map(|p| p.deny_keys())
                .unwrap_or_else(|| vec!["Escape".to_string()]);
            data::tmux::send_keys(&req.pane_id, &keys).await
        }
        PaneCommand::SendText(text) => data::tmux::send_text(&req.pane_id, text).await,
    };

    match result {
        Ok(()) => {
            let (message, new_activity) = match &req.command {
                PaneCommand::Approve => (
                    format!("Approved: {}", req.project_name),
                    AgentActivity::Processing,
                ),
                PaneCommand::Deny => (
                    format!("Denied: {}", req.project_name),
                    AgentActivity::WaitingForInput,
                ),
                PaneCommand::SendText(_) => (
                    format!("Sent prompt to {}", req.project_name),
                    AgentActivity::Processing,
                ),
            };
            info!(
                session_id = req.session_id,
                pane_id = req.pane_id,
                command = ?req.command,
                "executed pane action"
            );
            app.status_message = Some(message);

            // Optimistic update so the dashboard reflects the action now;
            // the next sync cycle confirms or corrects it.
            app.activity_map
                .insert(req.session_id.clone(), new_activity.clone());
            for session in app
                .active_sessions
                .iter_mut()
                .chain(app.all_sessions.iter_mut())
            {
                if session.session_id == req.session_id {
                    session.activity = new_activity.clone();
                }
            }
        }
        Err(e) => {
            warn!(error = %e, pane_id = req.pane_id, "pane action failed");
            app.last_error = Some(format!("Pane action failed: {e}"));
        }
    }
}

async fn run_file_watcher(
    dirs: Vec<PathBuf>,
    tx: mpsc::UnboundedSender<SyncMsg>,
    claude_dirs: Vec<PathBuf>,
    registry: Arc<AgentRegistry>,
    db_path: PathBuf,
    retention_days: Option<u32>,
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

    for dir in &dirs {
        // Recursive because the `projects/` tree has per-project subdirs
        // containing the JSONL transcripts we want to pick up.
        if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
            warn!(dir = %dir.display(), error = %e, "failed to watch dir");
        } else {
            info!(dir = %dir.display(), "watching for session file changes");
        }
    }

    let debounce = Duration::from_millis(500);
    let mut last_sync = std::time::Instant::now();

    let cfg = Config {
        claude_dirs,
        db_path: db_path.clone(),
        log_path: PathBuf::new(),
        poll_interval_secs: 0,
        tick_rate_ms: 0,
        notifications_enabled: false,
        transcript_retention_days: retention_days,
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

        // Also re-ingest transcripts on file change. Uses its own connection
        // to avoid contention with the main loop's connection.
        if let Ok(ingest_conn) = rusqlite::Connection::open(&db_path) {
            if let Err(e) = sync::ingest_transcripts(&cfg, &ingest_conn, &registry) {
                warn!(error = %e, "watch-triggered transcript ingest failed");
            }
        }

        last_sync = std::time::Instant::now();
    }

    Ok(())
}

mod routes;
mod static_files;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::Router;
use axum::routing::get;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use crate::agent::AgentRegistry;
use crate::config::Config;
use crate::data::sync;
use crate::db::schema;

/// Shared state available to all API handlers.
#[derive(Clone)]
pub struct WebState {
    pub db_path: PathBuf,
    pub claude_dirs: Vec<PathBuf>,
}

/// Build the axum router. Shared by the standalone and embedded modes.
fn build_router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/api/v1/sessions", get(routes::sessions))
        .route("/api/v1/sessions/active", get(routes::active_sessions))
        .route("/api/v1/projects", get(routes::projects))
        .route("/api/v1/history", get(routes::history))
        .route("/api/v1/stats", get(routes::stats))
        .route("/api/v1/conversations", get(routes::conversations))
        .route("/api/v1/conversations/{session_id}", get(routes::conversation_messages))
        .route("/api/v1/review", get(routes::review))
        .fallback(static_files::static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Start the HTTP server only, assuming the DB is already being populated by
/// another process (the TUI main loop). Intended for `--web` mode where TUI
/// + web run together.
pub async fn run_embedded(config: &Config, _registry: Arc<AgentRegistry>, port: u16) -> Result<()> {
    let state = Arc::new(WebState {
        db_path: config.db_path.clone(),
        claude_dirs: config.claude_dirs.clone(),
    });
    let app = build_router(state);
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!(port, "embedded web server listening on http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Start the background sync loop that writes directly to its own DB connection,
/// then start the axum HTTP server.
pub async fn run(config: &Config, registry: Arc<AgentRegistry>, port: u16) -> Result<()> {
    let db_path = config.db_path.clone();

    // Background sync task — owns its own DB connection
    let sync_config = Config {
        claude_dirs: config.claude_dirs.clone(),
        db_path: config.db_path.clone(),
        log_path: PathBuf::new(),
        poll_interval_secs: config.poll_interval_secs,
        tick_rate_ms: 0,
        notifications_enabled: false,
        transcript_retention_days: config.transcript_retention_days,
    };
    let sync_registry = Arc::clone(&registry);

    tokio::spawn(async move {
        let poll = Duration::from_secs(sync_config.poll_interval_secs);
        loop {
            match sync::gather_data(&sync_config, &sync_registry).await {
                Ok(data) => {
                    // Open a connection just for writing
                    match rusqlite::Connection::open(&sync_config.db_path) {
                        Ok(conn) => {
                            if let Err(e) = sync::apply_to_db(&conn, &data) {
                                error!(error = %e, "web sync: failed to write to DB");
                            }
                            // Hot-reload transcripts on the same connection
                            if let Err(e) =
                                sync::ingest_transcripts(&sync_config, &conn, &sync_registry)
                            {
                                error!(error = %e, "web sync: transcript ingest failed");
                            }
                        }
                        Err(e) => error!(error = %e, "web sync: failed to open DB"),
                    }
                }
                Err(e) => error!(error = %e, "web sync: gather_data failed"),
            }
            tokio::time::sleep(poll).await;
        }
    });

    let state = Arc::new(WebState {
        db_path,
        claude_dirs: config.claude_dirs.clone(),
    });

    let app = build_router(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!(port, "web server listening on http://localhost:{port}");

    // Run initial sync before serving
    {
        let conn = rusqlite::Connection::open(&config.db_path)?;
        schema::run_migrations(&conn)?;
        sync::import_history(config, &conn)?;
        if let Err(e) = sync::ingest_transcripts(config, &conn, &registry) {
            error!(error = %e, "initial transcript ingest failed");
        }
        let _ = sync::full_sync(config, &conn, &registry).await;
    }

    axum::serve(listener, app).await?;
    Ok(())
}

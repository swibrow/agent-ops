use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::data::claude;
use crate::db::queries;
use crate::model::session::{AgentSession, AgentType};
use crate::model::project::Project;
use crate::model::history::HistoryEntry;
use super::WebState;

// ── API response types ──────────────────────────────────────────

#[derive(Serialize)]
pub struct ApiSession {
    pub session_id: String,
    pub agent_type: String,
    pub agent_label: String,
    pub agent_icon: String,
    pub project_path: String,
    pub project_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: String,
    pub first_prompt: Option<String>,
    pub summary: Option<String>,
    pub git_branch: Option<String>,
    pub message_count: u32,
    pub last_activity: Option<i64>,
}

impl From<&AgentSession> for ApiSession {
    fn from(s: &AgentSession) -> Self {
        Self {
            session_id: s.session_id.clone(),
            agent_type: s.agent_type.as_str().to_string(),
            agent_label: s.agent_type.label().to_string(),
            agent_icon: s.agent_type.icon().to_string(),
            project_path: s.project_path.clone(),
            project_name: s.project_name.clone(),
            started_at: s.started_at,
            ended_at: s.ended_at,
            status: s.status.as_str().to_string(),
            first_prompt: s.first_prompt.clone(),
            summary: s.summary.clone(),
            git_branch: s.git_branch.clone(),
            message_count: s.message_count,
            last_activity: s.last_activity,
        }
    }
}

#[derive(Serialize)]
pub struct ApiProject {
    pub path: String,
    pub name: String,
    pub first_seen: i64,
    pub last_activity: i64,
    pub total_sessions: u32,
    pub total_messages: u32,
    pub is_active: bool,
    pub staleness: String,
    pub staleness_indicator: String,
    pub daily_activity: Vec<u64>,
}

impl From<&Project> for ApiProject {
    fn from(p: &Project) -> Self {
        Self {
            path: p.path.clone(),
            name: p.name.clone(),
            first_seen: p.first_seen,
            last_activity: p.last_activity,
            total_sessions: p.total_sessions,
            total_messages: p.total_messages,
            is_active: p.is_active,
            staleness: p.staleness.label().to_string(),
            staleness_indicator: p.staleness.indicator().to_string(),
            daily_activity: p.daily_activity.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct ApiHistory {
    pub timestamp: i64,
    pub project: String,
    pub display: String,
    pub session_id: Option<String>,
}

impl From<&HistoryEntry> for ApiHistory {
    fn from(e: &HistoryEntry) -> Self {
        Self {
            timestamp: e.timestamp,
            project: e.project.clone(),
            display: e.display.clone(),
            session_id: e.session_id.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct ApiStats {
    pub active_sessions: usize,
    pub total_sessions: usize,
    pub active_projects: usize,
    pub total_projects: usize,
    pub agent_type_counts: Vec<AgentTypeCount>,
}

#[derive(Serialize)]
pub struct AgentTypeCount {
    pub agent_type: String,
    pub label: String,
    pub count: usize,
}

// ── Helpers ─────────────────────────────────────────────────────

fn open_db(state: &WebState) -> Result<Connection, StatusCode> {
    Connection::open_with_flags(
        &state.db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ── Route handlers ──────────────────────────────────────────────

pub async fn sessions(
    State(state): State<Arc<WebState>>,
) -> Result<Json<Vec<ApiSession>>, StatusCode> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_db(&state)?;
        let sessions = queries::get_all_sessions(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(sessions.iter().map(ApiSession::from).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn active_sessions(
    State(state): State<Arc<WebState>>,
) -> Result<Json<Vec<ApiSession>>, StatusCode> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_db(&state)?;
        let sessions = queries::get_active_sessions(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(sessions.iter().map(ApiSession::from).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn projects(
    State(state): State<Arc<WebState>>,
) -> Result<Json<Vec<ApiProject>>, StatusCode> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_db(&state)?;
        let mut projects = queries::get_all_projects(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for project in &mut projects {
            if let Ok(activity) = queries::get_daily_activity(&conn, &project.path, 30) {
                project.daily_activity = activity;
            }
        }
        Ok(Json(projects.iter().map(ApiProject::from).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

#[derive(Deserialize)]
pub struct HistoryParams {
    pub project: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    500
}

pub async fn history(
    State(state): State<Arc<WebState>>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<ApiHistory>>, StatusCode> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_db(&state)?;
        let entries = queries::get_history(&conn, params.limit, params.project.as_deref())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(entries.iter().map(ApiHistory::from).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn stats(
    State(state): State<Arc<WebState>>,
) -> Result<Json<ApiStats>, StatusCode> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_db(&state)?;
        let all_sessions = queries::get_all_sessions(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let active_sessions = queries::get_active_sessions(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let projects = queries::get_all_projects(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let active_projects = projects.iter().filter(|p| p.is_active).count();

        // Count by agent type
        let mut type_counts = std::collections::HashMap::new();
        for s in &active_sessions {
            *type_counts.entry(s.agent_type).or_insert(0usize) += 1;
        }
        let agent_type_counts: Vec<AgentTypeCount> = AgentType::all()
            .iter()
            .filter_map(|at| {
                let count = type_counts.get(at).copied().unwrap_or(0);
                if count > 0 {
                    Some(AgentTypeCount {
                        agent_type: at.as_str().to_string(),
                        label: at.label().to_string(),
                        count,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(Json(ApiStats {
            active_sessions: active_sessions.len(),
            total_sessions: all_sessions.len(),
            active_projects,
            total_projects: projects.len(),
            agent_type_counts,
        }))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

// ── Conversation types ──────────────────────────────────────────

#[derive(Serialize)]
pub struct ApiConversation {
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub first_prompt: Option<String>,
    pub summary: Option<String>,
    pub message_count: Option<u32>,
    pub git_branch: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}

#[derive(Serialize)]
pub struct ApiChatMessage {
    pub role: String,
    pub content: Vec<ApiContentBlock>,
    pub timestamp: Option<String>,
    pub model: Option<String>,
}

#[derive(Serialize)]
pub struct ApiContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<String>,
}

// ── Conversation route handlers ─────────────────────────────────

/// List all conversations across all claude dirs (from sessions-index.json files).
pub async fn conversations(
    State(state): State<Arc<WebState>>,
) -> Result<Json<Vec<ApiConversation>>, StatusCode> {
    let claude_dirs = state.claude_dirs.clone();
    tokio::task::spawn_blocking(move || {
        let mut all = Vec::new();
        for claude_dir in &claude_dirs {
            if let Ok(projects) = claude::read_project_sessions(claude_dir) {
                for (project_path, entries) in projects {
                    let project_name = claude::project_name_from_path(&project_path);
                    for entry in entries {
                        all.push(ApiConversation {
                            session_id: entry.session_id,
                            project_path: project_path.clone(),
                            project_name: project_name.clone(),
                            first_prompt: entry.first_prompt,
                            summary: entry.summary,
                            message_count: entry.message_count,
                            git_branch: entry.git_branch,
                            created: entry.created,
                            modified: entry.modified,
                        });
                    }
                }
            }
        }
        // Sort by modified time, most recent first
        all.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(Json(all))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

/// Read the JSONL file for a specific conversation and return parsed chat messages.
pub async fn conversation_messages(
    State(state): State<Arc<WebState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ApiChatMessage>>, StatusCode> {
    let claude_dirs = state.claude_dirs.clone();
    tokio::task::spawn_blocking(move || {
        // Find the JSONL file across all claude dirs/projects
        let jsonl_path = find_conversation_file(&claude_dirs, &session_id)
            .ok_or(StatusCode::NOT_FOUND)?;

        let content = std::fs::read_to_string(&jsonl_path)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let mut messages = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let msg_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match msg_type {
                "user" => {
                    if let Some(msg) = parse_user_message(&value) {
                        messages.push(msg);
                    }
                }
                "assistant" => {
                    if let Some(msg) = parse_assistant_message(&value) {
                        messages.push(msg);
                    }
                }
                _ => {} // skip system, file-history-snapshot, etc.
            }
        }
        Ok(Json(messages))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

fn find_conversation_file(claude_dirs: &[std::path::PathBuf], session_id: &str) -> Option<std::path::PathBuf> {
    // Validate session_id looks like a UUID to prevent path traversal
    if !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    for claude_dir in claude_dirs {
        let projects_dir = claude_dir.join("projects");
        if !projects_dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let jsonl = entry.path().join(format!("{session_id}.jsonl"));
                if jsonl.exists() {
                    return Some(jsonl);
                }
            }
        }
    }
    None
}

fn parse_user_message(value: &serde_json::Value) -> Option<ApiChatMessage> {
    let message = value.get("message")?;
    let content_val = message.get("content")?;
    let timestamp = value.get("timestamp").and_then(|t| t.as_str()).map(String::from);

    let content = if let Some(text) = content_val.as_str() {
        // Simple text prompt
        vec![ApiContentBlock {
            block_type: "text".to_string(),
            text: Some(text.to_string()),
            tool_name: None,
            tool_input: None,
            tool_result: None,
        }]
    } else if let Some(arr) = content_val.as_array() {
        // Tool results array
        arr.iter()
            .filter_map(|block| {
                let btype = block.get("type")?.as_str()?;
                if btype == "tool_result" {
                    let result_text = block
                        .get("content")
                        .and_then(|c| {
                            if let Some(s) = c.as_str() {
                                Some(s.to_string())
                            } else if let Some(arr) = c.as_array() {
                                // content can be array of {type: "text", text: "..."}
                                Some(
                                    arr.iter()
                                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                )
                            } else {
                                None
                            }
                        });
                    Some(ApiContentBlock {
                        block_type: "tool_result".to_string(),
                        text: None,
                        tool_name: None,
                        tool_input: None,
                        tool_result: result_text,
                    })
                } else {
                    None
                }
            })
            .collect()
    } else {
        return None;
    };

    // Skip messages that are only tool results (noise in the chat view)
    if content.iter().all(|c| c.block_type == "tool_result") && !content.is_empty() {
        return None;
    }

    Some(ApiChatMessage {
        role: "user".to_string(),
        content,
        timestamp,
        model: None,
    })
}

fn parse_assistant_message(value: &serde_json::Value) -> Option<ApiChatMessage> {
    let message = value.get("message")?;
    let content_arr = message.get("content")?.as_array()?;
    let timestamp = value.get("timestamp").and_then(|t| t.as_str()).map(String::from);
    let model = message.get("model").and_then(|m| m.as_str()).map(String::from);

    let content: Vec<ApiContentBlock> = content_arr
        .iter()
        .filter_map(|block| {
            let btype = block.get("type")?.as_str()?;
            match btype {
                "text" => {
                    let text = block.get("text")?.as_str()?;
                    if text.is_empty() {
                        return None;
                    }
                    Some(ApiContentBlock {
                        block_type: "text".to_string(),
                        text: Some(text.to_string()),
                        tool_name: None,
                        tool_input: None,
                        tool_result: None,
                    })
                }
                "tool_use" => {
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                    let input = block.get("input").map(|i| {
                        // Summarize tool input — show first ~200 chars
                        let s = serde_json::to_string_pretty(i).unwrap_or_default();
                        if s.len() > 200 {
                            format!("{}...", &s[..200])
                        } else {
                            s
                        }
                    });
                    Some(ApiContentBlock {
                        block_type: "tool_use".to_string(),
                        text: None,
                        tool_name: Some(name.to_string()),
                        tool_input: input,
                        tool_result: None,
                    })
                }
                _ => None, // skip thinking blocks etc.
            }
        })
        .collect();

    if content.is_empty() {
        return None;
    }

    Some(ApiChatMessage {
        role: "assistant".to_string(),
        content,
        timestamp,
        model,
    })
}

// ── Review ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReviewParams {
    /// Unix milliseconds. Defaults to 24h ago.
    pub from: Option<i64>,
    /// Unix milliseconds. Defaults to now.
    pub to: Option<i64>,
    /// Optional project name or path substring filter.
    pub project: Option<String>,
    /// Max key prompts to include per session.
    pub max_prompts: Option<usize>,
}

#[derive(Serialize)]
pub struct ApiReview {
    pub range: ApiReviewRange,
    pub totals: ApiReviewTotals,
    pub projects: Vec<ApiReviewProject>,
}

#[derive(Serialize)]
pub struct ApiReviewRange {
    pub from: i64,
    pub to: i64,
    pub label: String,
}

#[derive(Serialize)]
pub struct ApiReviewTotals {
    pub projects: usize,
    pub sessions: usize,
    pub messages: usize,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub agents: std::collections::HashMap<String, usize>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

#[derive(Serialize)]
pub struct ApiReviewProject {
    pub project_path: String,
    pub project_name: String,
    pub session_count: usize,
    pub message_count: usize,
    pub first_activity: i64,
    pub last_activity: i64,
    pub agents: Vec<String>,
    pub branches: Vec<String>,
    /// One entry per session in this project, sorted by start time.
    pub sessions: Vec<ApiReviewSession>,
}

#[derive(Serialize)]
pub struct ApiReviewSession {
    pub session_id: String,
    pub agent_type: String,
    pub started_at: i64,
    pub last_activity: i64,
    pub message_count: usize,
    pub first_prompt: Option<String>,
    pub summary: Option<String>,
    pub git_branch: Option<String>,
    /// First few user prompts in chronological order.
    pub key_prompts: Vec<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

fn label_range(from: i64, to: i64) -> String {
    let span_hours = (to - from) / 3_600_000;
    if span_hours <= 1 {
        "last hour".to_string()
    } else if span_hours <= 24 {
        format!("last {} hours", span_hours.max(1))
    } else if span_hours <= 24 * 7 {
        format!("last {} days", span_hours / 24)
    } else {
        format!("last {} days", span_hours / 24)
    }
}

pub async fn review(
    State(state): State<Arc<WebState>>,
    Query(params): Query<ReviewParams>,
) -> Result<Json<ApiReview>, StatusCode> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_db(&state)?;

        let now = chrono::Utc::now().timestamp_millis();
        let to = params.to.unwrap_or(now);
        let from = params.from.unwrap_or(to - 86_400_000); // default: last 24h
        let max_prompts = params.max_prompts.unwrap_or(5).max(1).min(20);

        let summary = crate::db::queries::build_review(
            &conn,
            from,
            to,
            params.project.as_deref(),
            max_prompts,
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let projects: Vec<ApiReviewProject> = summary
            .projects
            .iter()
            .map(|p| ApiReviewProject {
                project_path: p.project_path.clone(),
                project_name: p.project_name.clone(),
                session_count: p.session_count,
                message_count: p.message_count,
                first_activity: p.first_activity,
                last_activity: p.last_activity,
                agents: p.agents.clone(),
                branches: p.branches.clone(),
                sessions: p
                    .sessions
                    .iter()
                    .map(|s| ApiReviewSession {
                        session_id: s.session_id.clone(),
                        agent_type: s.agent_type.clone(),
                        started_at: s.started_at,
                        last_activity: s.last_activity,
                        message_count: s.message_count,
                        first_prompt: s.first_prompt.clone(),
                        summary: s.summary.clone(),
                        git_branch: s.git_branch.clone(),
                        key_prompts: s.key_prompts.clone(),
                        input_tokens: s.input_tokens,
                        output_tokens: s.output_tokens,
                        cache_creation_tokens: s.cache_creation_tokens,
                        cache_read_tokens: s.cache_read_tokens,
                    })
                    .collect(),
            })
            .collect();

        Ok(Json(ApiReview {
            range: ApiReviewRange {
                from,
                to,
                label: label_range(from, to),
            },
            totals: ApiReviewTotals {
                projects: projects.len(),
                sessions: projects.iter().map(|p| p.session_count).sum(),
                messages: summary.total_messages,
                user_messages: summary.user_messages,
                assistant_messages: summary.assistant_messages,
                agents: summary.agents.clone(),
                input_tokens: summary.total_input_tokens,
                output_tokens: summary.total_output_tokens,
                cache_creation_tokens: summary.total_cache_creation_tokens,
                cache_read_tokens: summary.total_cache_read_tokens,
            },
            projects,
        }))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

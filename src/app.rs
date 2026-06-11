use rusqlite::Connection;
use tracing::warn;

use std::collections::HashMap;

use crate::data::stats::ProcessStats;
use crate::data::sync::{ActivityMap, SyncResult};
use crate::db::queries;
use crate::event::action::Action;
use crate::model::history::HistoryEntry;
use crate::model::project::{Project, ProjectSort, StalenessLevel};
use crate::model::review::{ReviewRange, ReviewSummary};
use crate::model::session::AgentActivity;
use crate::model::session::AgentSession;
use crate::model::session::AgentType;

/// What to do to a live tmux pane. Queued by `apply_action` (which stays
/// free of I/O) and executed asynchronously by the main loop.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneCommand {
    Approve,
    Deny,
    SendText(String),
}

/// A side-effect request targeting a live agent pane.
#[derive(Debug, Clone)]
pub struct PaneRequest {
    pub session_id: String,
    pub pane_id: String,
    pub agent_type: AgentType,
    pub project_name: String,
    pub command: PaneCommand,
}

/// Side effects queued by `apply_action` and executed by the main loop, so
/// key handling never blocks on process spawning or tmux I/O.
#[derive(Debug, Clone)]
pub enum AppCommand {
    Pane(PaneRequest),
    Resume(Box<AgentSession>),
    CopyToClipboard(String),
    OpenEditor { name: String, path: String },
}

/// The pane a prompt-input overlay is writing to.
#[derive(Debug, Clone)]
pub struct PromptTarget {
    pub session_id: String,
    pub pane_id: String,
    pub agent_type: AgentType,
    pub project_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveView {
    Dashboard,
    Projects,
    History,
    Review,
}

impl ActiveView {
    pub fn next(&self) -> Self {
        match self {
            Self::Dashboard => Self::Projects,
            Self::Projects => Self::Review,
            Self::Review => Self::History,
            Self::History => Self::Dashboard,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Dashboard => Self::History,
            Self::Projects => Self::Dashboard,
            Self::Review => Self::Projects,
            Self::History => Self::Review,
        }
    }
}

pub struct App {
    pub active_view: ActiveView,
    pub should_quit: bool,
    pub tick_count: u64,

    // Data
    pub active_sessions: Vec<AgentSession>,
    pub all_sessions: Vec<AgentSession>,
    pub projects: Vec<Project>,
    pub filtered_projects: Vec<Project>,
    pub history_entries: Vec<HistoryEntry>,
    pub global_daily_activity: Vec<u64>,

    // Live ephemeral state (not persisted in DB)
    pub activity_map: ActivityMap,
    pub stats_map: HashMap<String, ProcessStats>,
    pub self_stats: ProcessStats,

    // Counts
    pub active_pane_count: usize,
    pub active_project_count: usize,
    pub total_project_count: usize,

    // Dashboard state
    pub dashboard_selected: usize,
    pub dashboard_scroll: usize,

    // Projects state
    pub project_selected: usize,
    pub project_scroll: usize,
    pub project_sort: ProjectSort,
    pub project_filter_forgotten: bool,

    // History state
    pub history_selected: usize,
    pub history_scroll: usize,
    pub history_filter_project: Option<String>,

    // Review state
    pub review_range: ReviewRange,
    pub review_summary: Option<ReviewSummary>,
    pub review_selected: usize,
    pub review_scroll: usize,
    pub review_dirty: bool,
    pub review_expanded: std::collections::HashSet<String>,

    // Overlays
    pub show_detail: bool,
    pub show_help: bool,
    pub show_quit_confirm: bool,
    pub show_preview: bool,
    pub pane_preview: Option<String>,

    // Search
    pub search_active: bool,
    pub search_query: String,

    // Prompt input (write text back to an agent pane)
    pub prompt_active: bool,
    pub prompt_buffer: String,
    pub prompt_target: Option<PromptTarget>,

    // Side effects queued for the main loop to execute
    pub pending_commands: Vec<AppCommand>,

    // Agent filter (None = show all, Some(type) = show only that type)
    pub agent_filter: Option<AgentType>,

    // Status
    pub last_error: Option<String>,
    pub status_message: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            active_view: ActiveView::Dashboard,
            should_quit: false,
            tick_count: 0,
            active_sessions: Vec::new(),
            all_sessions: Vec::new(),
            projects: Vec::new(),
            filtered_projects: Vec::new(),
            history_entries: Vec::new(),
            global_daily_activity: Vec::new(),
            activity_map: ActivityMap::new(),
            stats_map: HashMap::new(),
            self_stats: ProcessStats::default(),
            active_pane_count: 0,
            active_project_count: 0,
            total_project_count: 0,
            dashboard_selected: 0,
            dashboard_scroll: 0,
            project_selected: 0,
            project_scroll: 0,
            project_sort: ProjectSort::LastActivity,
            project_filter_forgotten: false,
            history_selected: 0,
            history_scroll: 0,
            history_filter_project: None,
            review_range: ReviewRange::LastDay,
            review_summary: None,
            review_selected: 0,
            review_scroll: 0,
            review_dirty: true,
            review_expanded: std::collections::HashSet::new(),
            show_detail: false,
            show_help: false,
            show_quit_confirm: false,
            show_preview: false,
            pane_preview: None,
            search_active: false,
            search_query: String::new(),
            prompt_active: false,
            prompt_buffer: String::new(),
            prompt_target: None,
            pending_commands: Vec::new(),
            agent_filter: None,
            last_error: None,
            status_message: None,
        }
    }

    pub fn apply_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::RequestQuit => self.show_quit_confirm = true,
            Action::CancelQuit => self.show_quit_confirm = false,
            Action::SwitchTab(view) => {
                if view == ActiveView::Review && self.active_view != ActiveView::Review {
                    self.review_dirty = true;
                }
                self.active_view = view;
                self.show_detail = false;
                self.show_preview = false;
                self.pane_preview = None;
            }
            Action::NextTab => {
                self.active_view = self.active_view.next();
                self.show_detail = false;
                self.show_preview = false;
                self.pane_preview = None;
                if self.active_view == ActiveView::Review {
                    self.review_dirty = true;
                }
            }
            Action::PrevTab => {
                self.active_view = self.active_view.prev();
                self.show_detail = false;
                self.show_preview = false;
                self.pane_preview = None;
                if self.active_view == ActiveView::Review {
                    self.review_dirty = true;
                }
            }
            Action::SelectNext => self.select_next(),
            Action::SelectPrev => self.select_prev(),
            Action::SelectFirst => self.select_first(),
            Action::SelectLast => self.select_last(),
            Action::Confirm => {
                self.show_detail = true;
            }
            Action::TogglePreview => {
                self.show_preview = !self.show_preview;
                self.pane_preview = None; // reloaded by the main loop
            }
            Action::CycleSort => {
                self.project_sort = self.project_sort.next();
                self.sort_projects();
            }
            Action::FilterStale => match self.active_view {
                ActiveView::Projects => {
                    self.project_filter_forgotten = !self.project_filter_forgotten;
                    self.filter_projects();
                }
                ActiveView::History => {
                    if let Some(session) = self.selected_session() {
                        self.history_filter_project = Some(session.project_name.clone());
                    }
                }
                _ => {}
            },
            Action::ClearFilter => {
                self.project_filter_forgotten = false;
                self.history_filter_project = None;
                self.agent_filter = None;
                self.search_query.clear();
                self.filter_projects();
            }
            Action::CycleAgentFilter => {
                self.agent_filter = match self.agent_filter {
                    None => Some(AgentType::Claude),
                    Some(current) => {
                        let all = AgentType::all();
                        let idx = all.iter().position(|t| *t == current);
                        match idx {
                            Some(i) if i + 1 < all.len() => Some(all[i + 1]),
                            _ => None, // wrap back to "all"
                        }
                    }
                };
            }
            Action::ResumeSession => {
                if let Some(session) = self.selected_session() {
                    self.pending_commands
                        .push(AppCommand::Resume(Box::new(session.clone())));
                }
            }
            Action::CloseOverlay => {
                if self.show_help {
                    self.show_help = false;
                } else if self.show_preview {
                    self.show_preview = false;
                    self.pane_preview = None;
                } else if self.show_detail {
                    self.show_detail = false;
                    self.pane_preview = None;
                }
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::EnterSearch => {
                self.search_active = true;
                self.search_query.clear();
            }
            Action::ExitSearch => {
                self.search_active = false;
                self.filter_projects();
            }
            Action::SearchInput(c) => {
                self.search_query.push(c);
                self.filter_projects();
            }
            Action::SearchBackspace => {
                self.search_query.pop();
                self.filter_projects();
            }
            Action::CopySessionId => {
                if let Some(session) = self.selected_session() {
                    self.pending_commands
                        .push(AppCommand::CopyToClipboard(session.session_id.clone()));
                }
            }
            Action::ApprovePermission => {
                self.queue_permission_response(PaneCommand::Approve);
            }
            Action::DenyPermission => {
                self.queue_permission_response(PaneCommand::Deny);
            }
            Action::EnterPrompt => {
                self.enter_prompt();
            }
            Action::PromptInput(c) => {
                if self.prompt_active {
                    self.prompt_buffer.push(c);
                }
            }
            Action::PromptBackspace => {
                self.prompt_buffer.pop();
            }
            Action::SubmitPrompt => {
                self.submit_prompt();
            }
            Action::CancelPrompt => {
                self.prompt_active = false;
                self.prompt_buffer.clear();
                self.prompt_target = None;
            }
            Action::OpenInEditor => {
                if let Some(project) = self.selected_project() {
                    self.pending_commands.push(AppCommand::OpenEditor {
                        name: project.name.clone(),
                        path: project.path.clone(),
                    });
                }
            }
            Action::Tick => {
                self.tick_count += 1;
                // Clear status message after ~3 seconds (60 ticks at 50ms)
                if self.tick_count.is_multiple_of(60) {
                    self.status_message = None;
                }
            }
            Action::CycleReviewRange => {
                self.review_range = self.review_range.next();
                self.review_dirty = true;
                self.review_expanded.clear();
                self.review_selected = 0;
                self.review_scroll = 0;
            }
            Action::CycleReviewRangeBack => {
                self.review_range = self.review_range.prev();
                self.review_dirty = true;
                self.review_expanded.clear();
                self.review_selected = 0;
                self.review_scroll = 0;
            }
            Action::ToggleReviewExpand => {
                if let Some(summary) = &self.review_summary {
                    if let Some(project) = summary.projects.get(self.review_selected) {
                        let key = project.project_path.clone();
                        if self.review_expanded.contains(&key) {
                            self.review_expanded.remove(&key);
                        } else {
                            self.review_expanded.insert(key);
                        }
                    }
                }
            }
            Action::Refresh => {
                self.review_dirty = true;
            }
            Action::DataRefreshed | Action::None => {}
        }
    }

    /// Load the review summary for the current range from the DB.
    /// Sets `review_dirty = false`. Safe to call on every frame — cheap when
    /// nothing's changed because the caller guards it with `review_dirty`.
    pub fn load_review(&mut self, conn: &Connection) {
        let now = chrono::Utc::now().timestamp_millis();
        let from = now - self.review_range.millis();
        match queries::build_review(conn, from, now, None, 5) {
            Ok(summary) => {
                self.review_summary = Some(summary);
                self.review_selected = 0;
                self.review_scroll = 0;
            }
            Err(e) => {
                warn!(error = %e, "build_review failed");
                self.last_error = Some(format!("Review query failed: {e}"));
            }
        }
        self.review_dirty = false;
    }

    pub fn refresh_from_db(&mut self, conn: &Connection) {
        self.active_sessions = queries::get_active_sessions(conn).unwrap_or_default();
        self.all_sessions = queries::get_all_sessions(conn).unwrap_or_default();

        let mut projects = queries::get_all_projects(conn).unwrap_or_default();
        for project in &mut projects {
            project.daily_activity =
                queries::get_daily_activity(conn, &project.path, 30).unwrap_or_default();
        }
        self.projects = projects;

        self.history_entries =
            queries::get_history(conn, 500, self.history_filter_project.as_deref())
                .unwrap_or_default();

        self.active_pane_count = self.active_sessions.len();
        self.active_project_count = self.projects.iter().filter(|p| p.is_active).count();
        self.total_project_count = self.projects.len();

        // Compute global daily activity (sum across all projects)
        self.global_daily_activity = vec![0u64; 30];
        for project in &self.projects {
            for (i, &val) in project.daily_activity.iter().enumerate() {
                if i < 30 {
                    self.global_daily_activity[i] += val;
                }
            }
        }

        // Overlay live activity + stats onto sessions loaded from DB
        for session in &mut self.active_sessions {
            if let Some(activity) = self.activity_map.get(&session.session_id) {
                session.activity = activity.clone();
            }
            if let Some(ps) = self.stats_map.get(&session.session_id) {
                session.cpu_percent = ps.cpu_percent;
                session.memory_mb = ps.memory_mb;
            }
        }
        for session in &mut self.all_sessions {
            if let Some(activity) = self.activity_map.get(&session.session_id) {
                session.activity = activity.clone();
            }
            if let Some(ps) = self.stats_map.get(&session.session_id) {
                session.cpu_percent = ps.cpu_percent;
                session.memory_mb = ps.memory_mb;
            }
        }

        // Sort active sessions: needs-attention first, then processing, then idle.
        // Within the same activity state, sort by project name for stability.
        self.active_sessions.sort_by(|a, b| {
            a.activity
                .sort_priority()
                .cmp(&b.activity.sort_priority())
                .then_with(|| a.project_name.cmp(&b.project_name))
        });

        // Clear any previous error on successful refresh
        self.last_error = None;

        self.sort_projects();
        self.filter_projects();

        // New data may have landed; refresh review next time it's visible
        self.review_dirty = true;
    }

    pub fn apply_sync_result(&mut self, result: &SyncResult) {
        self.activity_map = result.activity_map.clone();
        self.stats_map = result.stats_map.clone();
        self.self_stats = result.self_stats.clone();
    }

    pub fn selected_session(&self) -> Option<&AgentSession> {
        match self.active_view {
            ActiveView::Dashboard => self.active_sessions.get(self.dashboard_selected),
            ActiveView::Projects => {
                let project = self.filtered_projects.get(self.project_selected)?;
                self.active_sessions
                    .iter()
                    .find(|s| s.project_path == project.path)
                    .or_else(|| {
                        self.all_sessions
                            .iter()
                            .find(|s| s.project_path == project.path)
                    })
            }
            ActiveView::History => {
                let entry = self.history_entries.get(self.history_selected)?;
                self.all_sessions.iter().find(|s| {
                    s.session_id == entry.session_id.as_deref().unwrap_or("")
                        || s.project_path == entry.project
                })
            }
            ActiveView::Review => None,
        }
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.filtered_projects.get(self.project_selected)
    }

    fn select_next(&mut self) {
        if self.show_preview {
            self.pane_preview = None; // preview follows the selection
        }
        match self.active_view {
            ActiveView::Dashboard => {
                if self.dashboard_selected < self.active_sessions.len().saturating_sub(1) {
                    self.dashboard_selected += 1;
                    self.adjust_dashboard_scroll();
                }
            }
            ActiveView::Projects => {
                if self.project_selected < self.filtered_projects.len().saturating_sub(1) {
                    self.project_selected += 1;
                    self.adjust_project_scroll();
                }
            }
            ActiveView::History => {
                if self.history_selected < self.history_entries.len().saturating_sub(1) {
                    self.history_selected += 1;
                    self.adjust_history_scroll();
                }
            }
            ActiveView::Review => {
                let total = self.review_summary
                    .as_ref()
                    .map(|s| s.projects.len())
                    .unwrap_or(0);
                if self.review_selected < total.saturating_sub(1) {
                    self.review_selected += 1;
                    self.adjust_review_scroll();
                }
            }
        }
    }

    fn select_prev(&mut self) {
        if self.show_preview {
            self.pane_preview = None; // preview follows the selection
        }
        match self.active_view {
            ActiveView::Dashboard => {
                self.dashboard_selected = self.dashboard_selected.saturating_sub(1);
                self.adjust_dashboard_scroll();
            }
            ActiveView::Projects => {
                self.project_selected = self.project_selected.saturating_sub(1);
                self.adjust_project_scroll();
            }
            ActiveView::History => {
                self.history_selected = self.history_selected.saturating_sub(1);
                self.adjust_history_scroll();
            }
            ActiveView::Review => {
                self.review_selected = self.review_selected.saturating_sub(1);
                self.adjust_review_scroll();
            }
        }
    }

    fn select_first(&mut self) {
        match self.active_view {
            ActiveView::Dashboard => {
                self.dashboard_selected = 0;
                self.dashboard_scroll = 0;
            }
            ActiveView::Projects => {
                self.project_selected = 0;
                self.project_scroll = 0;
            }
            ActiveView::History => {
                self.history_selected = 0;
                self.history_scroll = 0;
            }
            ActiveView::Review => {
                self.review_selected = 0;
                self.review_scroll = 0;
            }
        }
    }

    fn select_last(&mut self) {
        match self.active_view {
            ActiveView::Dashboard => {
                self.dashboard_selected = self.active_sessions.len().saturating_sub(1);
                self.adjust_dashboard_scroll();
            }
            ActiveView::Projects => {
                self.project_selected = self.filtered_projects.len().saturating_sub(1);
                self.adjust_project_scroll();
            }
            ActiveView::Review => {
                let total = self.review_summary
                    .as_ref()
                    .map(|s| s.projects.len())
                    .unwrap_or(0);
                self.review_selected = total.saturating_sub(1);
                self.adjust_review_scroll();
            }
            ActiveView::History => {
                self.history_selected = self.history_entries.len().saturating_sub(1);
                self.adjust_history_scroll();
            }
        }
    }

    fn adjust_dashboard_scroll(&mut self) {
        let visible = 5;
        if self.dashboard_selected < self.dashboard_scroll {
            self.dashboard_scroll = self.dashboard_selected;
        } else if self.dashboard_selected >= self.dashboard_scroll + visible {
            self.dashboard_scroll = self.dashboard_selected - visible + 1;
        }
    }

    fn adjust_project_scroll(&mut self) {
        let visible = 7;
        if self.project_selected < self.project_scroll {
            self.project_scroll = self.project_selected;
        } else if self.project_selected >= self.project_scroll + visible {
            self.project_scroll = self.project_selected - visible + 1;
        }
    }

    fn adjust_history_scroll(&mut self) {
        let visible = 15;
        if self.history_selected < self.history_scroll {
            self.history_scroll = self.history_selected;
        } else if self.history_selected >= self.history_scroll + visible {
            self.history_scroll = self.history_selected - visible + 1;
        }
    }

    fn adjust_review_scroll(&mut self) {
        let visible = 10;
        if self.review_selected < self.review_scroll {
            self.review_scroll = self.review_selected;
        } else if self.review_selected >= self.review_scroll + visible {
            self.review_scroll = self.review_selected - visible + 1;
        }
    }

    fn sort_projects(&mut self) {
        self.projects.sort_by(|a, b| match self.project_sort {
            ProjectSort::Name => a.name.cmp(&b.name),
            ProjectSort::LastActivity => b.last_activity.cmp(&a.last_activity),
            ProjectSort::Sessions => b.total_sessions.cmp(&a.total_sessions),
            ProjectSort::Staleness => a.staleness.cmp(&b.staleness),
        });
    }

    fn filter_projects(&mut self) {
        let query = self.search_query.to_lowercase();

        self.filtered_projects = self
            .projects
            .iter()
            .filter(|p| {
                // Apply forgotten filter
                if self.project_filter_forgotten && p.staleness != StalenessLevel::Forgotten {
                    return false;
                }
                // Apply search filter
                if !query.is_empty() {
                    let name_match = p.name.to_lowercase().contains(&query);
                    let path_match = p.path.to_lowercase().contains(&query);
                    return name_match || path_match;
                }
                true
            })
            .cloned()
            .collect();

        // Reset selection if out of bounds
        if self.project_selected >= self.filtered_projects.len() {
            self.project_selected = self.filtered_projects.len().saturating_sub(1);
        }
    }

    /// Queue an approve/deny for the selected session's permission prompt.
    /// Gated on the session actually waiting for permission so a stray
    /// keypress can't inject keys into a working agent.
    fn queue_permission_response(&mut self, command: PaneCommand) {
        let session = match self.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };
        let pane = match &session.tmux_pane {
            Some(p) => p.clone(),
            None => {
                self.status_message = Some("No live tmux pane for this session".to_string());
                return;
            }
        };
        if session.activity != AgentActivity::WaitingForPermission {
            self.status_message = Some(format!(
                "{} isn't waiting for permission",
                session.project_name
            ));
            return;
        }

        self.pending_commands.push(AppCommand::Pane(PaneRequest {
            session_id: session.session_id.clone(),
            pane_id: pane.pane_id,
            agent_type: session.agent_type,
            project_name: session.project_name.clone(),
            command,
        }));
    }

    /// Open the prompt-input overlay targeting the selected session's pane.
    fn enter_prompt(&mut self) {
        let session = match self.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };
        let pane = match &session.tmux_pane {
            Some(p) => p.clone(),
            None => {
                self.status_message = Some("No live tmux pane for this session".to_string());
                return;
            }
        };

        self.prompt_active = true;
        self.prompt_buffer.clear();
        self.prompt_target = Some(PromptTarget {
            session_id: session.session_id.clone(),
            pane_id: pane.pane_id,
            agent_type: session.agent_type,
            project_name: session.project_name.clone(),
        });
    }

    /// Queue the typed prompt text for delivery to the target pane.
    fn submit_prompt(&mut self) {
        let target = match self.prompt_target.take() {
            Some(t) => t,
            None => {
                self.prompt_active = false;
                return;
            }
        };
        let text = std::mem::take(&mut self.prompt_buffer);
        self.prompt_active = false;

        if text.trim().is_empty() {
            return;
        }

        self.pending_commands.push(AppCommand::Pane(PaneRequest {
            session_id: target.session_id,
            pane_id: target.pane_id,
            agent_type: target.agent_type,
            project_name: target.project_name,
            command: PaneCommand::SendText(text),
        }));
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::session::SessionStatus;
    use crate::model::tmux::TmuxPaneRef;

    fn session_with_pane(activity: AgentActivity) -> AgentSession {
        AgentSession {
            session_id: "s1".to_string(),
            pid: Some(1),
            agent_type: AgentType::Claude,
            project_path: "/tmp/proj".to_string(),
            project_name: "proj".to_string(),
            started_at: 0,
            ended_at: None,
            status: SessionStatus::Active,
            first_prompt: None,
            summary: None,
            git_branch: None,
            message_count: 0,
            last_activity: None,
            tmux_pane: Some(TmuxPaneRef {
                session_name: "main".to_string(),
                window_index: 0,
                pane_id: "%5".to_string(),
            }),
            pane_title: None,
            activity,
            cpu_percent: 0.0,
            memory_mb: 0.0,
        }
    }

    fn pane_request(command: &AppCommand) -> &PaneRequest {
        match command {
            AppCommand::Pane(req) => req,
            other => panic!("expected pane command, got {other:?}"),
        }
    }

    #[test]
    fn approve_queues_request_when_waiting_for_permission() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForPermission));

        app.apply_action(Action::ApprovePermission);

        assert_eq!(app.pending_commands.len(), 1);
        let req = pane_request(&app.pending_commands[0]);
        assert_eq!(req.pane_id, "%5");
        assert_eq!(req.command, PaneCommand::Approve);
    }

    #[test]
    fn approve_is_gated_on_permission_state() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::Processing));

        app.apply_action(Action::ApprovePermission);

        assert!(app.pending_commands.is_empty());
        assert!(app.status_message.is_some());
    }

    #[test]
    fn deny_queues_request_when_waiting_for_permission() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForPermission));

        app.apply_action(Action::DenyPermission);

        assert_eq!(app.pending_commands.len(), 1);
        assert_eq!(
            pane_request(&app.pending_commands[0]).command,
            PaneCommand::Deny
        );
    }

    #[test]
    fn prompt_flow_queues_send_text() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForInput));

        app.apply_action(Action::EnterPrompt);
        assert!(app.prompt_active);

        for c in "fix the tests".chars() {
            app.apply_action(Action::PromptInput(c));
        }
        app.apply_action(Action::SubmitPrompt);

        assert!(!app.prompt_active);
        assert_eq!(app.pending_commands.len(), 1);
        assert_eq!(
            pane_request(&app.pending_commands[0]).command,
            PaneCommand::SendText("fix the tests".to_string())
        );
    }

    #[test]
    fn empty_prompt_is_not_sent() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForInput));

        app.apply_action(Action::EnterPrompt);
        app.apply_action(Action::SubmitPrompt);

        assert!(!app.prompt_active);
        assert!(app.pending_commands.is_empty());
    }

    #[test]
    fn cancel_prompt_clears_state() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForInput));

        app.apply_action(Action::EnterPrompt);
        app.apply_action(Action::PromptInput('x'));
        app.apply_action(Action::CancelPrompt);

        assert!(!app.prompt_active);
        assert!(app.prompt_buffer.is_empty());
        assert!(app.prompt_target.is_none());
        assert!(app.pending_commands.is_empty());
    }

    #[test]
    fn resume_queues_resume_command() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForInput));

        app.apply_action(Action::ResumeSession);

        assert_eq!(app.pending_commands.len(), 1);
        assert!(matches!(
            &app.pending_commands[0],
            AppCommand::Resume(s) if s.session_id == "s1"
        ));
    }

    #[test]
    fn copy_queues_clipboard_command() {
        let mut app = App::new();
        app.active_sessions
            .push(session_with_pane(AgentActivity::WaitingForInput));

        app.apply_action(Action::CopySessionId);

        assert!(matches!(
            &app.pending_commands[0],
            AppCommand::CopyToClipboard(text) if text == "s1"
        ));
    }

    #[test]
    fn prompt_without_pane_shows_status() {
        let mut app = App::new();
        let mut session = session_with_pane(AgentActivity::WaitingForInput);
        session.tmux_pane = None;
        app.active_sessions.push(session);

        app.apply_action(Action::EnterPrompt);

        assert!(!app.prompt_active);
        assert!(app.status_message.is_some());
    }
}

use rusqlite::Connection;
use tracing::{info, warn};

use std::collections::HashMap;

use crate::data::stats::ProcessStats;
use crate::data::sync::{ActivityMap, SyncResult};
use crate::db::queries;
use crate::event::action::Action;
use crate::model::history::HistoryEntry;
use crate::model::project::{Project, ProjectSort, StalenessLevel};
use crate::model::session::AgentSession;

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveView {
    Dashboard,
    Projects,
    History,
}

impl ActiveView {
    pub fn next(&self) -> Self {
        match self {
            Self::Dashboard => Self::Projects,
            Self::Projects => Self::History,
            Self::History => Self::Dashboard,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Dashboard => Self::History,
            Self::Projects => Self::Dashboard,
            Self::History => Self::Projects,
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

    // Overlays
    pub show_detail: bool,
    pub show_help: bool,
    pub show_quit_confirm: bool,
    pub pane_preview: Option<String>,

    // Search
    pub search_active: bool,
    pub search_query: String,

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
            show_detail: false,
            show_help: false,
            show_quit_confirm: false,
            pane_preview: None,
            search_active: false,
            search_query: String::new(),
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
                self.active_view = view;
                self.show_detail = false;
                self.pane_preview = None;
            }
            Action::NextTab => {
                self.active_view = self.active_view.next();
                self.show_detail = false;
                self.pane_preview = None;
            }
            Action::PrevTab => {
                self.active_view = self.active_view.prev();
                self.show_detail = false;
                self.pane_preview = None;
            }
            Action::SelectNext => self.select_next(),
            Action::SelectPrev => self.select_prev(),
            Action::SelectFirst => self.select_first(),
            Action::SelectLast => self.select_last(),
            Action::Confirm => {
                self.show_detail = true;
            }
            Action::TogglePreview => {
                self.pane_preview = None; // will be loaded on next tick
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
                self.search_query.clear();
                self.filter_projects();
            }
            Action::ResumeSession => {
                self.build_resume_command();
            }
            Action::CloseOverlay => {
                if self.show_help {
                    self.show_help = false;
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
                self.copy_session_id();
            }
            Action::OpenInEditor => {
                self.open_in_editor();
            }
            Action::Tick => {
                self.tick_count += 1;
                // Clear status message after ~3 seconds (60 ticks at 50ms)
                if self.tick_count.is_multiple_of(60) {
                    self.status_message = None;
                }
            }
            Action::Refresh | Action::DataRefreshed | Action::None => {}
        }
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
        }
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.filtered_projects.get(self.project_selected)
    }

    fn select_next(&mut self) {
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
        }
    }

    fn select_prev(&mut self) {
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

    fn build_resume_command(&mut self) {
        let session = match self.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };

        let commands: Vec<Vec<String>> = if let Some(ref pane) = session.tmux_pane {
            vec![
                vec![
                    "tmux".to_string(),
                    "select-window".to_string(),
                    "-t".to_string(),
                    format!("{}:{}", pane.session_name, pane.window_index),
                ],
                vec![
                    "tmux".to_string(),
                    "select-pane".to_string(),
                    "-t".to_string(),
                    pane.pane_id.clone(),
                ],
            ]
        } else {
            vec![vec![
                "tmux".to_string(),
                "new-window".to_string(),
                "-n".to_string(),
                session.project_name.clone(),
                "-c".to_string(),
                session.project_path.clone(),
                "claude".to_string(),
                "--resume".to_string(),
                session.session_id.clone(),
            ]]
        };

        for cmd in &commands {
            if cmd.is_empty() {
                continue;
            }
            match std::process::Command::new(&cmd[0]).args(&cmd[1..]).status() {
                Ok(status) if !status.success() => {
                    warn!(cmd = ?cmd, "tmux command failed");
                    self.last_error = Some(format!("tmux command failed: {}", cmd.join(" ")));
                }
                Err(e) => {
                    warn!(error = %e, "failed to execute tmux command");
                    self.last_error = Some(format!("Failed to run tmux: {e}"));
                }
                _ => {
                    info!(session_id = session.session_id, "resumed session");
                    self.status_message = Some(format!("Resumed: {}", session.project_name));
                }
            }
        }
    }

    fn copy_session_id(&mut self) {
        let session = match self.selected_session() {
            Some(s) => s,
            None => return,
        };

        let session_id = session.session_id.clone();
        match std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.take() {
                    use std::io::Write;
                    let mut stdin = stdin;
                    let _ = stdin.write_all(session_id.as_bytes());
                }
                let _ = child.wait();
                self.status_message = Some(format!("Copied: {session_id}"));
            }
            Err(e) => {
                warn!(error = %e, "failed to copy to clipboard");
                self.last_error = Some("Failed to copy to clipboard".to_string());
            }
        }
    }

    fn open_in_editor(&mut self) {
        let project = match self.selected_project() {
            Some(p) => p.clone(),
            None => return,
        };

        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
        match std::process::Command::new(&editor)
            .arg(&project.path)
            .spawn()
        {
            Ok(_) => {
                self.status_message = Some(format!("Opened {} in {editor}", project.name));
            }
            Err(e) => {
                warn!(error = %e, editor, "failed to open editor");
                self.last_error = Some(format!("Failed to open {editor}: {e}"));
            }
        }
    }
}

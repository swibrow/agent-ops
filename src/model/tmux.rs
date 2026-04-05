#[derive(Debug, Clone)]
pub struct TmuxPane {
    pub id: String,
    pub pid: u32,
    pub title: String,
    #[allow(dead_code)]
    pub current_command: String,
    pub current_path: String,
    /// Whether this pane is the active pane in its window.
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct TmuxPaneRef {
    pub session_name: String,
    pub window_index: u32,
    pub pane_id: String,
}

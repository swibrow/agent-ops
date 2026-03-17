use crate::model::tmux::TmuxPaneRef;

/// What the agent is actively doing right now
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AgentActivity {
    /// Agent is thinking/running tools (braille spinner in pane title)
    Processing,
    /// Agent is waiting for user text input (❯ prompt visible)
    WaitingForInput,
    /// Agent is waiting for permission (e.g. "Do you want to proceed?")
    WaitingForPermission,
    #[default]
    /// Unknown or not applicable
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Active,
    Idle,
    Completed,
    Abandoned,
    Unknown,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "idle" => Self::Idle,
            "completed" => Self::Completed,
            "abandoned" => Self::Abandoned,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentSession {
    pub session_id: String,
    pub pid: Option<u32>,
    pub project_path: String,
    pub project_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: SessionStatus,
    pub first_prompt: Option<String>,
    pub summary: Option<String>,
    pub git_branch: Option<String>,
    pub message_count: u32,
    pub last_activity: Option<i64>,
    pub tmux_pane: Option<TmuxPaneRef>,
    pub pane_title: Option<String>,
    pub activity: AgentActivity,
    pub cpu_percent: f32,
    pub memory_mb: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SessionStatus::from_str / as_str round-trip ──────────────

    #[test]
    fn session_status_round_trip_active() {
        let status = SessionStatus::Active;
        assert_eq!(
            SessionStatus::from_str(status.as_str()),
            SessionStatus::Active
        );
    }

    #[test]
    fn session_status_round_trip_idle() {
        let status = SessionStatus::Idle;
        assert_eq!(
            SessionStatus::from_str(status.as_str()),
            SessionStatus::Idle
        );
    }

    #[test]
    fn session_status_round_trip_completed() {
        let status = SessionStatus::Completed;
        assert_eq!(
            SessionStatus::from_str(status.as_str()),
            SessionStatus::Completed
        );
    }

    #[test]
    fn session_status_round_trip_abandoned() {
        let status = SessionStatus::Abandoned;
        assert_eq!(
            SessionStatus::from_str(status.as_str()),
            SessionStatus::Abandoned
        );
    }

    #[test]
    fn session_status_round_trip_unknown() {
        let status = SessionStatus::Unknown;
        assert_eq!(
            SessionStatus::from_str(status.as_str()),
            SessionStatus::Unknown
        );
    }

    #[test]
    fn session_status_from_str_unrecognized_yields_unknown() {
        assert_eq!(SessionStatus::from_str("garbage"), SessionStatus::Unknown);
        assert_eq!(SessionStatus::from_str(""), SessionStatus::Unknown);
        assert_eq!(SessionStatus::from_str("Active"), SessionStatus::Unknown); // case-sensitive
    }

    #[test]
    fn session_status_as_str_values() {
        assert_eq!(SessionStatus::Active.as_str(), "active");
        assert_eq!(SessionStatus::Idle.as_str(), "idle");
        assert_eq!(SessionStatus::Completed.as_str(), "completed");
        assert_eq!(SessionStatus::Abandoned.as_str(), "abandoned");
        assert_eq!(SessionStatus::Unknown.as_str(), "unknown");
    }

    // ── AgentActivity default ────────────────────────────────────

    #[test]
    fn agent_activity_default_is_unknown() {
        assert_eq!(AgentActivity::default(), AgentActivity::Unknown);
    }
}

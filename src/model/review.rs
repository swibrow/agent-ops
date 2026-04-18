//! Review model: shared by the TUI and web routes so both code paths read
//! from the same shape.

use std::collections::HashMap;

/// A preset time range for the review view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRange {
    LastHour,
    LastDay,
    LastWeek,
    LastMonth,
}

impl ReviewRange {
    pub fn millis(self) -> i64 {
        match self {
            Self::LastHour => 3_600_000,
            Self::LastDay => 86_400_000,
            Self::LastWeek => 604_800_000,
            Self::LastMonth => 2_592_000_000,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LastHour => "Last hour",
            Self::LastDay => "Last 24 hours",
            Self::LastWeek => "Last 7 days",
            Self::LastMonth => "Last 30 days",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::LastHour => "1h",
            Self::LastDay => "24h",
            Self::LastWeek => "7d",
            Self::LastMonth => "30d",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::LastHour => Self::LastDay,
            Self::LastDay => Self::LastWeek,
            Self::LastWeek => Self::LastMonth,
            Self::LastMonth => Self::LastHour,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::LastHour => Self::LastMonth,
            Self::LastDay => Self::LastHour,
            Self::LastWeek => Self::LastDay,
            Self::LastMonth => Self::LastWeek,
        }
    }
}

/// A session that appeared in a review range.
#[derive(Debug, Clone)]
#[allow(dead_code)] // several fields are read only by the web route
pub struct ReviewSession {
    pub session_id: String,
    pub agent_type: String,
    pub started_at: i64,
    pub last_activity: i64,
    pub message_count: usize,
    pub first_prompt: Option<String>,
    pub summary: Option<String>,
    pub git_branch: Option<String>,
    pub key_prompts: Vec<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

/// All activity within a single project for the review range.
#[derive(Debug, Clone)]
#[allow(dead_code)] // first_activity + agents consumed by web route only
pub struct ReviewProject {
    pub project_path: String,
    pub project_name: String,
    pub session_count: usize,
    pub message_count: usize,
    pub first_activity: i64,
    pub last_activity: i64,
    pub agents: Vec<String>,
    pub branches: Vec<String>,
    pub sessions: Vec<ReviewSession>,
}

/// Top-level aggregate for a given time range.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // from/to/agents consumed by web route only
pub struct ReviewSummary {
    pub from: i64,
    pub to: i64,
    pub total_messages: usize,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub agents: HashMap<String, usize>,
    pub projects: Vec<ReviewProject>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_creation_tokens: i64,
    pub total_cache_read_tokens: i64,
}

impl ReviewSummary {
    pub fn session_count(&self) -> usize {
        self.projects.iter().map(|p| p.session_count).sum()
    }

    pub fn project_count(&self) -> usize {
        self.projects.len()
    }
}

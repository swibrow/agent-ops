pub mod aider;
pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::config::Config;
use crate::model::session::{AgentActivity, AgentType};
use crate::model::tmux::TmuxPane;

/// A session file read from an agent's data directory (e.g., ~/.claude/sessions/).
/// Only agents that persist session files (currently Claude) return these.
#[derive(Debug, Clone)]
pub struct AgentSessionFile {
    pub session_id: String,
    pub pid: Option<u32>,
    pub cwd: String,
    pub started_at: i64,
}

/// Trait for detecting and gathering data about a specific CLI agent.
///
/// Sync methods (detect_pane, detect_activity, clean_title) are called per-pane
/// during detection — they must be fast and non-blocking.
///
/// Async methods (gather_sessions, refine_activity) are called once per sync
/// cycle and may do I/O.
pub trait AgentProvider: Send + Sync {
    /// Which agent type this provider handles.
    fn agent_type(&self) -> AgentType;

    /// Check if a tmux pane belongs to this agent.
    /// The registry pre-filters shell panes before calling this.
    fn detect_pane(&self, pane: &TmuxPane) -> bool;

    /// Detect activity state from pane title/metadata (no I/O).
    fn detect_activity(&self, pane: &TmuxPane) -> AgentActivity;

    /// Strip agent-specific prefixes from the pane title for display.
    fn clean_title<'a>(&self, title: &'a str) -> &'a str;

    /// Read session files from the agent's data directory.
    /// Return empty vec if the agent doesn't persist session files.
    fn gather_sessions(
        &self,
        config: &Config,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<AgentSessionFile>>> + Send + '_>>;

    /// Refine activity detection with I/O (e.g., pane content sniffing).
    /// Default: return base activity unchanged.
    fn refine_activity(
        &self,
        _pane_id: &str,
        base: AgentActivity,
    ) -> Pin<Box<dyn Future<Output = AgentActivity> + Send + '_>> {
        Box::pin(async move { base })
    }
}

/// Known shell commands — panes running these are pre-filtered before
/// any provider gets to see them.
const SHELLS: &[&str] = &["zsh", "bash", "fish", "sh", "dash", "ksh", "tcsh", "csh"];

/// Registry of all agent providers. First-match-wins detection.
pub struct AgentRegistry {
    providers: Vec<Box<dyn AgentProvider>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn register(&mut self, provider: Box<dyn AgentProvider>) {
        self.providers.push(provider);
    }

    /// Build the default registry with all known providers.
    /// Order matters: more specific detectors first (Claude checks title prefix),
    /// then command-based detectors.
    pub fn default_registry() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(claude::ClaudeProvider));
        registry.register(Box::new(codex::CodexProvider));
        registry.register(Box::new(opencode::OpenCodeProvider));
        registry.register(Box::new(gemini::GeminiProvider));
        registry.register(Box::new(aider::AiderProvider));
        registry
    }

    /// Detect which agent (if any) is running in the given pane.
    /// Returns None if the pane is running a shell or no provider matches.
    pub fn detect(&self, pane: &TmuxPane) -> Option<&dyn AgentProvider> {
        // Pre-filter: skip shell panes
        if SHELLS.contains(&pane.current_command.as_str()) {
            return None;
        }

        self.providers
            .iter()
            .find(|p| p.detect_pane(pane))
            .map(|p| p.as_ref())
    }

    /// Get all registered providers.
    pub fn providers(&self) -> &[Box<dyn AgentProvider>] {
        &self.providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pane(command: &str, title: &str) -> TmuxPane {
        TmuxPane {
            id: "%1".to_string(),
            pid: 1234,
            title: title.to_string(),
            current_command: command.to_string(),
            current_path: "/tmp".to_string(),
            active: false,
        }
    }

    #[test]
    fn registry_detects_claude_by_title() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("node", "\u{2733} Claude Code");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::Claude);
    }

    #[test]
    fn registry_detects_codex_by_command() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("codex", "Codex CLI");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::Codex);
    }

    #[test]
    fn registry_detects_opencode_by_command() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("opencode", "OpenCode");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::OpenCode);
    }

    #[test]
    fn registry_detects_gemini_by_command() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("gemini", "Gemini CLI");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::Gemini);
    }

    #[test]
    fn registry_detects_aider_by_command() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("aider", "Aider");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::Aider);
    }

    #[test]
    fn registry_skips_shell_panes() {
        let reg = AgentRegistry::default_registry();
        // Even if title looks like Claude, shell pane is filtered
        let pane = make_pane("zsh", "\u{2733} Claude Code");
        assert!(reg.detect(&pane).is_none());
    }

    #[test]
    fn registry_skips_shell_panes_bash() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("bash", "some title");
        assert!(reg.detect(&pane).is_none());
    }

    #[test]
    fn registry_returns_none_for_unknown() {
        let reg = AgentRegistry::default_registry();
        let pane = make_pane("vim", "editing main.rs");
        assert!(reg.detect(&pane).is_none());
    }

    #[test]
    fn registry_first_match_wins_claude_over_command() {
        let reg = AgentRegistry::default_registry();
        // Claude provider checks title prefix; if title has sparkle, Claude wins
        // even if command is something else
        let pane = make_pane("node", "\u{2801} Processing");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::Claude);
    }

    #[test]
    fn registry_claude_by_command_name() {
        let reg = AgentRegistry::default_registry();
        // Claude also detected by command name (version string like "2.1.77")
        let pane = make_pane("claude", "plain title");
        let provider = reg.detect(&pane);
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().agent_type(), AgentType::Claude);
    }
}

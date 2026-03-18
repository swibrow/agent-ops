#[derive(Debug, Clone)]
pub struct TmuxPane {
    pub id: String,
    pub pid: u32,
    pub title: String,
    #[allow(dead_code)]
    pub current_command: String,
    pub current_path: String,
}

impl TmuxPane {
    pub fn is_claude_agent(&self) -> bool {
        let has_claude_title =
            self.title.starts_with('\u{2733}') // ✳ prefix (idle)
            || self.has_braille_prefix(); // braille spinner (processing)

        // A finished Claude session leaves the ✳ title on the pane even after
        // the process exits. Filter these out by checking if the pane is just
        // running a shell — if so, Claude has exited.
        has_claude_title && !self.is_shell()
    }

    /// Returns true if the pane is running a bare shell (agent has exited).
    fn is_shell(&self) -> bool {
        matches!(
            self.current_command.as_str(),
            "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh"
        )
    }

    /// Check if pane title starts with a braille character (U+2800-U+28FF)
    /// Claude Code uses braille spinner when processing
    fn has_braille_prefix(&self) -> bool {
        self.title
            .chars()
            .next()
            .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
    }

    /// Detect agent activity from pane title prefix
    pub fn detect_activity(&self) -> crate::model::session::AgentActivity {
        if self.has_braille_prefix() {
            crate::model::session::AgentActivity::Processing
        } else if self.title.starts_with('\u{2733}') {
            crate::model::session::AgentActivity::WaitingForInput
        } else {
            crate::model::session::AgentActivity::Unknown
        }
    }

    pub fn clean_title(&self) -> &str {
        let title = &self.title;
        // Strip braille spinner or sparkle prefix
        if let Some(first) = title.chars().next() {
            if ('\u{2800}'..='\u{28FF}').contains(&first) || first == '\u{2733}' {
                return title[first.len_utf8()..].trim();
            }
        }
        title
    }
}

#[derive(Debug, Clone)]
pub struct TmuxPaneRef {
    pub session_name: String,
    pub window_index: u32,
    pub pane_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::session::AgentActivity;

    fn make_pane(title: &str) -> TmuxPane {
        TmuxPane {
            id: "%1".to_string(),
            pid: 1234,
            title: title.to_string(),
            current_command: "node".to_string(),
            current_path: "/tmp".to_string(),
        }
    }

    // ── is_claude_agent ──────────────────────────────────────────

    #[test]
    fn is_claude_agent_with_sparkle_prefix() {
        let pane = make_pane("\u{2733} Doing something");
        assert!(pane.is_claude_agent());
    }

    #[test]
    fn is_claude_agent_with_braille_prefix() {
        // U+2801 is in the braille range U+2800..=U+28FF
        let pane = make_pane("\u{2801} Processing...");
        assert!(pane.is_claude_agent());
    }

    #[test]
    fn is_claude_agent_with_braille_range_start() {
        let pane = make_pane("\u{2800} Start of range");
        assert!(pane.is_claude_agent());
    }

    #[test]
    fn is_claude_agent_with_braille_range_end() {
        let pane = make_pane("\u{28FF} End of range");
        assert!(pane.is_claude_agent());
    }

    #[test]
    fn is_not_claude_agent_plain_title() {
        let pane = make_pane("vim src/main.rs");
        assert!(!pane.is_claude_agent());
    }

    #[test]
    fn is_not_claude_agent_empty_title() {
        let pane = make_pane("");
        assert!(!pane.is_claude_agent());
    }

    #[test]
    fn is_not_claude_agent_regular_unicode() {
        let pane = make_pane("🚀 deploying...");
        assert!(!pane.is_claude_agent());
    }

    #[test]
    fn is_not_claude_agent_char_just_outside_braille_range() {
        // U+27FF is just below braille range
        let pane = make_pane("\u{27FF} not braille");
        assert!(!pane.is_claude_agent());
        // U+2900 is just above braille range
        let pane2 = make_pane("\u{2900} also not braille");
        assert!(!pane2.is_claude_agent());
    }

    #[test]
    fn is_not_claude_agent_when_shell_took_over() {
        // Claude exited but pane still has the ✳ title — current_command is now zsh
        let pane = TmuxPane {
            id: "%1".to_string(),
            pid: 1234,
            title: "\u{2733} Claude Code".to_string(),
            current_command: "zsh".to_string(),
            current_path: "/tmp".to_string(),
        };
        assert!(!pane.is_claude_agent());
    }

    #[test]
    fn is_not_claude_agent_when_bash_took_over() {
        let pane = TmuxPane {
            id: "%1".to_string(),
            pid: 1234,
            title: "\u{2801} Processing".to_string(),
            current_command: "bash".to_string(),
            current_path: "/tmp".to_string(),
        };
        assert!(!pane.is_claude_agent());
    }

    // ── has_braille_prefix (tested via is_claude_agent) ──────────

    #[test]
    fn has_braille_prefix_mid_range() {
        let pane = make_pane("\u{2840} mid-range braille");
        assert!(pane.is_claude_agent());
    }

    // ── detect_activity ──────────────────────────────────────────

    #[test]
    fn detect_activity_processing_on_braille() {
        let pane = make_pane("\u{2801} working...");
        assert_eq!(pane.detect_activity(), AgentActivity::Processing);
    }

    #[test]
    fn detect_activity_waiting_on_sparkle() {
        let pane = make_pane("\u{2733} idle prompt");
        assert_eq!(pane.detect_activity(), AgentActivity::WaitingForInput);
    }

    #[test]
    fn detect_activity_unknown_on_plain() {
        let pane = make_pane("bash");
        assert_eq!(pane.detect_activity(), AgentActivity::Unknown);
    }

    #[test]
    fn detect_activity_unknown_on_empty() {
        let pane = make_pane("");
        assert_eq!(pane.detect_activity(), AgentActivity::Unknown);
    }

    // ── clean_title ──────────────────────────────────────────────

    #[test]
    fn clean_title_strips_sparkle() {
        let pane = make_pane("\u{2733} Claude session");
        assert_eq!(pane.clean_title(), "Claude session");
    }

    #[test]
    fn clean_title_strips_braille() {
        let pane = make_pane("\u{2801} Processing files");
        assert_eq!(pane.clean_title(), "Processing files");
    }

    #[test]
    fn clean_title_trims_whitespace() {
        let pane = make_pane("\u{2733}   lots of spaces  ");
        assert_eq!(pane.clean_title(), "lots of spaces");
    }

    #[test]
    fn clean_title_returns_plain_title_unmodified() {
        let pane = make_pane("vim main.rs");
        assert_eq!(pane.clean_title(), "vim main.rs");
    }

    #[test]
    fn clean_title_empty_after_prefix() {
        let pane = make_pane("\u{2733}");
        assert_eq!(pane.clean_title(), "");
    }

    #[test]
    fn clean_title_empty_input() {
        let pane = make_pane("");
        assert_eq!(pane.clean_title(), "");
    }
}

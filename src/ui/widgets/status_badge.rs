use ratatui::prelude::*;

use crate::model::session::{AgentActivity, AgentType};

/// Spinner frames for "processing" (braille dots like Claude Code uses)
const PROCESSING_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠇"];

/// Activity-aware badge: shows what the agent is currently doing
pub fn activity_span(activity: &AgentActivity, tick: u64) -> Span<'static> {
    match activity {
        AgentActivity::Processing => {
            let frame_idx = (tick / 3) as usize % PROCESSING_FRAMES.len();
            Span::styled(
                format!("{} ", PROCESSING_FRAMES[frame_idx]),
                Style::default().fg(Color::Magenta).bold(),
            )
        }
        AgentActivity::WaitingForInput => {
            let color = if (tick / 8).is_multiple_of(2) {
                Color::Green
            } else {
                Color::Rgb(50, 180, 50)
            };
            Span::styled("❯ ".to_string(), Style::default().fg(color))
        }
        AgentActivity::WaitingForPermission => {
            let (icon, color) = if (tick / 6).is_multiple_of(2) {
                ("⚡", Color::Yellow)
            } else {
                ("⚡", Color::Rgb(200, 150, 0))
            };
            Span::styled(format!("{icon} "), Style::default().fg(color).bold())
        }
        AgentActivity::Completed => {
            Span::styled("✅ ".to_string(), Style::default().fg(Color::Green).bold())
        }
        AgentActivity::Unknown => {
            let frame_idx = (tick / 8) as usize % 4;
            let colors = [
                Color::Green,
                Color::LightGreen,
                Color::Green,
                Color::LightGreen,
            ];
            Span::styled(
                "● ".to_string(),
                Style::default().fg(colors[frame_idx]).bold(),
            )
        }
    }
}

/// Agent type icon in the agent's brand color.
pub fn agent_type_span(agent_type: &AgentType) -> Span<'static> {
    Span::styled(
        format!("{} ", agent_type.icon()),
        Style::default().fg(agent_type.color()),
    )
}

/// Short text label for the activity state
pub fn activity_label(activity: &AgentActivity) -> Span<'static> {
    match activity {
        AgentActivity::Processing => {
            Span::styled("PROCESSING", Style::default().fg(Color::Magenta).bold())
        }
        AgentActivity::WaitingForInput => Span::styled("IDLE", Style::default().fg(Color::Green)),
        AgentActivity::WaitingForPermission => {
            Span::styled("NEEDS INPUT", Style::default().fg(Color::Yellow).bold())
        }
        AgentActivity::Completed => {
            Span::styled("DONE", Style::default().fg(Color::Green).bold())
        }
        AgentActivity::Unknown => Span::styled("ACTIVE", Style::default().fg(Color::Green)),
    }
}

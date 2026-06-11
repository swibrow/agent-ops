//! Live pane preview overlay: a near-fullscreen view of the selected
//! session's tmux pane, refreshed continuously by the main loop.

use ansi_to_tui::IntoText;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use crate::app::App;
use crate::model::session::AgentActivity;
use crate::ui::centered_rect;
use crate::ui::widgets::status_badge;

/// Parse ANSI-colored pane content into styled ratatui text.
/// Falls back to the raw string if the escape sequences don't parse.
pub fn ansi_text(content: &str) -> Text<'static> {
    content
        .into_text()
        .unwrap_or_else(|_| Text::raw(content.to_string()))
}

/// Trim trailing blank lines and return the last `max` lines.
pub fn tail<'a>(lines: &'a [Line<'static>], max: usize) -> &'a [Line<'static>] {
    let end = lines
        .iter()
        .rposition(|l| l.spans.iter().any(|s| !s.content.trim().is_empty()))
        .map(|i| i + 1)
        .unwrap_or(0);
    &lines[end.saturating_sub(max)..end]
}

pub fn draw(frame: &mut Frame, app: &App) {
    let session = match app.selected_session() {
        Some(s) => s,
        None => return,
    };

    let popup = centered_rect(90, 85, frame.area());
    frame.render_widget(Clear, popup);

    let activity_icon = status_badge::activity_span(&session.activity, app.tick_count);
    let activity_lbl = status_badge::activity_label(&session.activity);
    let title = Line::from(vec![
        Span::styled(
            format!(" {} ", session.project_name),
            Style::default().fg(session.agent_type.color()).bold(),
        ),
        activity_icon,
        activity_lbl,
        Span::raw(" "),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(session.agent_type.color()))
        .title(title)
        .padding(Padding::new(1, 1, 0, 0));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    match &app.pane_preview {
        Some(content) => {
            let text = ansi_text(content);
            let visible = tail(&text.lines, layout[0].height as usize).to_vec();
            frame.render_widget(Paragraph::new(Text::from(visible)), layout[0]);
        }
        None => {
            let placeholder = Line::from(Span::styled(
                if session.tmux_pane.is_some() {
                    "Capturing pane…"
                } else {
                    "No live tmux pane for this session"
                },
                Style::default().fg(Color::DarkGray).italic(),
            ));
            frame.render_widget(Paragraph::new(placeholder), layout[0]);
        }
    }

    let mut hints = Vec::new();
    if session.activity == AgentActivity::WaitingForPermission {
        hints.push(Span::styled("[a] ", Style::default().fg(Color::Green)));
        hints.push(Span::styled("Approve", Style::default().fg(Color::DarkGray)));
        hints.push(Span::styled("  [d] ", Style::default().fg(Color::Red)));
        hints.push(Span::styled("Deny", Style::default().fg(Color::DarkGray)));
        hints.push(Span::raw("  "));
    }
    hints.extend([
        Span::styled("[i] ", Style::default().fg(Color::Yellow)),
        Span::styled("Write", Style::default().fg(Color::DarkGray)),
        Span::styled("  [j/k] ", Style::default().fg(Color::Yellow)),
        Span::styled("Switch session", Style::default().fg(Color::DarkGray)),
        Span::styled("  [r] ", Style::default().fg(Color::Yellow)),
        Span::styled("Resume", Style::default().fg(Color::DarkGray)),
        Span::styled("  [Esc/Space] ", Style::default().fg(Color::Yellow)),
        Span::styled("Close", Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(Line::from(hints)), layout[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_text_parses_colored_content() {
        let text = ansi_text("\x1b[31mred\x1b[0m plain");
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans[0].content, "red");
        assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn tail_trims_trailing_blanks_and_limits() {
        let text = ansi_text("one\ntwo\nthree\n\n\n");
        let lines = tail(&text.lines, 2);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "two");
        assert_eq!(lines[1].spans[0].content, "three");
    }

    #[test]
    fn tail_of_empty_content_is_empty() {
        let text = ansi_text("");
        assert!(tail(&text.lines, 10).is_empty());
    }

    #[test]
    fn tail_ignores_blank_styled_lines() {
        // A trailing line holding only a color reset still counts as blank.
        let text = ansi_text("content\n\x1b[0m \n");
        let lines = tail(&text.lines, 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "content");
    }
}

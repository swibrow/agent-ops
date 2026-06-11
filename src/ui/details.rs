use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;
use crate::ui::widgets::{status_badge, time};

pub fn draw(frame: &mut Frame, app: &App) {
    let session = match app.selected_session() {
        Some(s) => s,
        None => return,
    };

    let popup_area = centered_rect(70, 75, frame.area());
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Session Detail ")
        .title_style(Style::default().fg(Color::Cyan).bold())
        .padding(Padding::new(2, 2, 1, 1));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let activity_icon = status_badge::activity_span(&session.activity, app.tick_count);
    let activity_lbl = status_badge::activity_label(&session.activity);
    let tmux_loc = session
        .tmux_pane
        .as_ref()
        .map(|p| format!("tmux {}:{} {}", p.session_name, p.window_index, p.pane_id))
        .unwrap_or_else(|| "detached".to_string());

    let started = time::format_datetime(session.started_at);
    let duration = time::format_duration(session.started_at);
    let duration_hours = time::duration_hours(session.started_at);
    let title = session.pane_title.as_deref().unwrap_or("N/A");
    let first_prompt = session.first_prompt.as_deref().unwrap_or("N/A");
    let summary = session.summary.as_deref().unwrap_or("");

    // Duration color based on age
    let duration_color = if duration_hours > 8.0 {
        Color::Red
    } else if duration_hours > 1.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    let max_text_width = inner.width as usize;

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Project:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &session.project_name,
                Style::default().fg(Color::Cyan).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Agent:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                session.agent_type.label(),
                Style::default().fg(session.agent_type.color()).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Branch:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                session.git_branch.as_deref().unwrap_or("N/A"),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Session:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&session.session_id, Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  Status:   ", Style::default().fg(Color::DarkGray)),
            activity_icon,
            activity_lbl,
            Span::styled(
                format!("  ({})", tmux_loc),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Title:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(title, Style::default().fg(Color::White).italic()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Started:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(started, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  Duration: ", Style::default().fg(Color::DarkGray)),
            Span::styled(duration, Style::default().fg(duration_color).bold()),
        ]),
        Line::from(vec![
            Span::styled("  Messages: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", session.message_count),
                Style::default().fg(Color::Green).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  CPU:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.1}%", session.cpu_percent),
                Style::default().fg(if session.cpu_percent > 50.0 {
                    Color::Red
                } else if session.cpu_percent > 10.0 {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
            Span::styled("    RAM: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.0} MB", session.memory_mb),
                Style::default().fg(if session.memory_mb > 512.0 {
                    Color::Red
                } else if session.memory_mb > 256.0 {
                    Color::Yellow
                } else {
                    Color::Cyan
                }),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  First prompt:",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                time::truncate_to_width(first_prompt, max_text_width.saturating_sub(4)),
                Style::default().fg(Color::White).italic(),
            ),
        ]),
    ];

    if !summary.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Summary:",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                time::truncate_to_width(summary, max_text_width.saturating_sub(4)),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    // Pane preview: the tail of the pane (where the prompt/permission box
    // lives), filling whatever vertical space is left in the popup.
    if let Some(preview) = &app.pane_preview {
        // Rows still free: header/footer chrome around the preview box is
        // 4 lines (blank, top rule, bottom rule, hint line at the end).
        let avail = (inner.height as usize)
            .saturating_sub(lines.len() + 4)
            .max(3);
        let rule_width = max_text_width.saturating_sub(4);

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "  ┌─ Pane Preview {}",
                "─".repeat(rule_width.saturating_sub(17))
            ),
            Style::default().fg(Color::DarkGray),
        )));
        let preview_text = crate::ui::preview::ansi_text(preview);
        for preview_line in crate::ui::preview::tail(&preview_text.lines, avail) {
            let mut spans = vec![Span::styled("  │ ", Style::default().fg(Color::DarkGray))];
            spans.extend(preview_line.spans.iter().cloned());
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(Span::styled(
            format!("  └{}", "─".repeat(rule_width.saturating_sub(3))),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let mut hint_spans = vec![
        Span::styled("  [r] ", Style::default().fg(Color::Yellow)),
        Span::styled("Resume", Style::default().fg(Color::DarkGray)),
        Span::styled("  [y] ", Style::default().fg(Color::Yellow)),
        Span::styled("Copy ID", Style::default().fg(Color::DarkGray)),
    ];
    if session.tmux_pane.is_some() {
        if session.activity == crate::model::session::AgentActivity::WaitingForPermission {
            hint_spans.push(Span::styled("  [a] ", Style::default().fg(Color::Green)));
            hint_spans.push(Span::styled("Approve", Style::default().fg(Color::DarkGray)));
            hint_spans.push(Span::styled("  [d] ", Style::default().fg(Color::Red)));
            hint_spans.push(Span::styled("Deny", Style::default().fg(Color::DarkGray)));
        }
        hint_spans.push(Span::styled("  [i] ", Style::default().fg(Color::Yellow)));
        hint_spans.push(Span::styled("Write", Style::default().fg(Color::DarkGray)));
    }
    hint_spans.push(Span::styled("  [Esc] ", Style::default().fg(Color::Yellow)));
    hint_spans.push(Span::styled("Close", Style::default().fg(Color::DarkGray)));

    lines.push(Line::from(""));
    lines.push(Line::from(hint_spans));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

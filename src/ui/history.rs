use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::App;
use crate::ui::widgets::{activity_spark, time};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // History list
            Constraint::Length(4), // Activity heatmap
        ])
        .split(area);

    draw_history_list(frame, layout[0], app);
    draw_heatmap(frame, layout[1], app);
}

fn draw_history_list(frame: &mut Frame, area: Rect, app: &App) {
    let filter_label = app
        .history_filter_project
        .as_ref()
        .map(|p| format!(" [{}]", p))
        .unwrap_or_default();
    let title = format!(" History{filter_label} ");

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title)
        .title_style(Style::default().fg(Color::Cyan).bold())
        .padding(Padding::new(1, 1, 0, 0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let entries = &app.history_entries;

    if entries.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  No history entries found",
            Style::default().fg(Color::DarkGray).italic(),
        )));
        frame.render_widget(empty, inner);
        return;
    }

    let visible_height = inner.height as usize;
    let scroll_offset = app.history_scroll;
    let selected = app.history_selected;

    let mut lines: Vec<Line> = Vec::new();
    let mut last_date = String::new();

    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        if lines.len() >= visible_height {
            break;
        }

        // Date separator
        let date = time::format_date(entry.timestamp);
        if date != last_date {
            let sep = Line::from(vec![
                Span::styled("  ── ", Style::default().fg(Color::DarkGray)),
                Span::styled(date.clone(), Style::default().fg(Color::White).bold()),
                Span::styled(
                    " ──────────────────────────────────────────",
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            lines.push(sep);
            last_date = date;

            if lines.len() >= visible_height {
                break;
            }
        }

        let is_selected = i == selected;
        let time_str = time::format_time(entry.timestamp);
        let project_name = extract_project_name(&entry.project);

        // Truncate display text
        let max_display_len = (inner.width as usize).saturating_sub(30);
        let display = time::truncate_to_width(&entry.display, max_display_len);

        let line = Line::from(vec![
            Span::raw(if is_selected { "▸ " } else { "  " }),
            Span::styled(
                format!("{time_str}  "),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:<14}", project_name),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("\"{display}\""),
                if is_selected {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]);

        lines.push(line);
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    if entries.len() > visible_height {
        let mut scrollbar_state = ScrollbarState::new(entries.len()).position(scroll_offset);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn draw_heatmap(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Activity Heatmap (30d) ")
        .title_style(Style::default().fg(Color::Cyan))
        .padding(Padding::new(1, 1, 0, 0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    let spark = activity_spark::spark_line(&app.global_daily_activity, width, false);

    let paragraph = Paragraph::new(vec![spark]);
    frame.render_widget(paragraph, inner);
}

fn extract_project_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

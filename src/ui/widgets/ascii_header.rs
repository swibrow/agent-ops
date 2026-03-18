use ratatui::prelude::*;

use crate::app::ActiveView;

const HEADER: [&str; 3] = [
    r"╔═╗ ╔═╗ ╔═╗ ╔╗╔ ╔╦╗   ╔═╗ ╔═╗ ╔═╗",
    r"╠═╣ ║ ╦ ║╣  ║║║  ║    ║ ║ ╠═╝ ╚═╗",
    r"╩ ╩ ╚═╝ ╚═╝ ╝╚╝  ╩    ╚═╝ ╩   ╚═╝",
];

/// Color gradient palette that shifts with tick
const GRADIENT: [Color; 8] = [
    Color::Rgb(147, 51, 234), // purple
    Color::Rgb(168, 85, 247), // lighter purple
    Color::Rgb(99, 102, 241), // indigo
    Color::Rgb(59, 130, 246), // blue
    Color::Rgb(6, 182, 212),  // cyan
    Color::Rgb(20, 184, 166), // teal
    Color::Rgb(34, 197, 94),  // green
    Color::Rgb(250, 204, 21), // yellow
];

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    tick: u64,
    active_view: &ActiveView,
    pane_count: usize,
) {
    let shift = (tick / 8) as usize;
    let width = area.width as usize;

    // Build the tab bar line (bottom of header)
    let tab_line = build_tab_line(active_view, pane_count, width);

    // Layout: 3 lines of ASCII art + 1 line tab bar
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // ASCII art
            Constraint::Length(1), // Tab bar
        ])
        .split(area);

    // Draw ASCII art centered
    let art_lines: Vec<Line> = HEADER
        .iter()
        .enumerate()
        .map(|(row_idx, line)| {
            let spans: Vec<Span> = line
                .chars()
                .enumerate()
                .map(|(col_idx, ch)| {
                    let color_idx = (col_idx / 3 + shift + row_idx) % GRADIENT.len();
                    Span::styled(
                        ch.to_string(),
                        Style::default().fg(GRADIENT[color_idx]).bold(),
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    let art = ratatui::widgets::Paragraph::new(art_lines).alignment(Alignment::Center);
    frame.render_widget(art, layout[0]);

    // Draw tab bar
    let tab_paragraph = ratatui::widgets::Paragraph::new(tab_line);
    frame.render_widget(tab_paragraph, layout[1]);
}

fn build_tab_line<'a>(active_view: &ActiveView, pane_count: usize, width: usize) -> Line<'a> {
    let tabs = [
        ("1", "Dashboard", ActiveView::Dashboard),
        ("2", "Projects", ActiveView::Projects),
        ("3", "History", ActiveView::History),
    ];

    let mut spans: Vec<Span> = vec![Span::raw(" ")];

    for (key, label, view) in &tabs {
        if active_view == view {
            spans.push(Span::styled(
                format!(" {key}:{label} "),
                Style::default().fg(Color::Cyan).bold(),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {key}:{label} "),
                Style::default().fg(Color::DarkGray),
            ));
        }
        spans.push(Span::styled("│", Style::default().fg(Color::Rgb(50, 50, 65))));
    }

    // Right-aligned agent count
    let agent_text = format!(" {} active ", pane_count);
    let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
    let pad = width.saturating_sub(left_len + agent_text.len());
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(
        agent_text,
        Style::default().fg(Color::Green).bold(),
    ));

    Line::from(spans)
}

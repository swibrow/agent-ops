use ratatui::prelude::*;

use crate::app::ActiveView;

const HEADER: [&str; 3] = [
    r"╔═╗ ╔═╗ ╔═╗ ╔╗╔ ╔╦╗   ╔═╗ ╔═╗ ╔═╗",
    r"╠═╣ ║ ╦ ║╣  ║║║  ║    ║ ║ ╠═╝ ╚═╗",
    r"╩ ╩ ╚═╝ ╚═╝ ╝╚╝  ╩    ╚═╝ ╩   ╚═╝",
];

const SUBTITLE: &str = "─── monitor ∙ track ∙ resume ───────";

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

pub fn draw(frame: &mut Frame, area: Rect, tick: u64, active_view: &ActiveView, pane_count: usize) {
    let shift = (tick / 8) as usize;
    let width = area.width as usize;

    // Layout: 3 lines ASCII art + 1 line subtitle with tabs
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // ASCII art
            Constraint::Length(1), // Subtitle + tabs
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

    // Bottom line: tabs | subtitle (centered) | agent count
    let tab_line = build_subtitle_line(active_view, pane_count, width, shift);
    let paragraph = ratatui::widgets::Paragraph::new(tab_line);
    frame.render_widget(paragraph, layout[1]);
}

fn build_subtitle_line<'a>(
    active_view: &ActiveView,
    pane_count: usize,
    width: usize,
    shift: usize,
) -> Line<'a> {
    let tabs = [
        ("1", "Dashboard", ActiveView::Dashboard),
        ("2", "Projects", ActiveView::Projects),
        ("3", "Review", ActiveView::Review),
        ("4", "History", ActiveView::History),
    ];

    // Build left: tabs
    let mut left_spans: Vec<Span> = vec![Span::raw(" ")];
    for (key, label, view) in &tabs {
        if active_view == view {
            left_spans.push(Span::styled(
                format!("{key}:{label}"),
                Style::default().fg(Color::Cyan).bold(),
            ));
        } else {
            left_spans.push(Span::styled(
                format!("{key}:{label}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
    }

    // Build center: subtitle with gradient
    let subtitle_spans: Vec<Span> = SUBTITLE
        .chars()
        .enumerate()
        .map(|(col_idx, ch)| {
            let color_idx = (col_idx / 3 + shift + 3) % GRADIENT.len();
            Span::styled(
                ch.to_string(),
                Style::default().fg(GRADIENT[color_idx]).bold(),
            )
        })
        .collect();

    // Build right: agent count
    let agent_text = format!("{} active ", pane_count);
    let right_span = Span::styled(agent_text.clone(), Style::default().fg(Color::Green).bold());

    // Calculate padding
    let left_len: usize = left_spans.iter().map(|s| s.content.len()).sum();
    let center_len = SUBTITLE.len();
    let right_len = agent_text.len();
    let total = left_len + center_len + right_len;
    let remaining = width.saturating_sub(total);
    let pad_left = remaining / 2;
    let pad_right = remaining.saturating_sub(pad_left);

    // Assemble
    let mut spans = left_spans;
    spans.push(Span::raw(" ".repeat(pad_left)));
    spans.extend(subtitle_spans);
    spans.push(Span::raw(" ".repeat(pad_right)));
    spans.push(right_span);

    Line::from(spans)
}

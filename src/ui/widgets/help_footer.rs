use ratatui::prelude::*;

use crate::app::ActiveView;

pub fn draw(frame: &mut Frame, area: Rect, view: &ActiveView) {
    let common_keys = vec![
        key_hint("r", "resume"),
        Span::raw("  "),
        key_hint("Enter", "details"),
        Span::raw("  "),
        key_hint("/", "search"),
        Span::raw("  "),
        key_hint("h/l", "tabs"),
        Span::raw("  "),
        key_hint("?", "help"),
        Span::raw("  "),
        key_hint("q", "quit"),
    ];

    let view_keys: Vec<Span> = match view {
        ActiveView::Dashboard => vec![
            key_hint("a/d", "approve/deny"),
            Span::raw("  "),
            key_hint("i", "write"),
            Span::raw("  "),
            key_hint("Space", "preview"),
            Span::raw("  "),
            key_hint("A", "agent"),
            Span::raw("  "),
        ],
        ActiveView::Projects => vec![
            key_hint("s", "sort"),
            Span::raw("  "),
            key_hint("f", "filter"),
            Span::raw("  "),
            key_hint("e", "editor"),
            Span::raw("  "),
        ],
        ActiveView::History => vec![key_hint("f", "filter"), Span::raw("  ")],
        ActiveView::Review => vec![
            key_hint("t/T", "range"),
            Span::raw("  "),
            key_hint("Enter", "expand"),
            Span::raw("  "),
        ],
    };

    let mut spans = vec![Span::raw(" ")];
    spans.extend(view_keys);
    spans.extend(common_keys);

    let line = Line::from(spans);
    let paragraph =
        ratatui::widgets::Paragraph::new(line).style(Style::default().bg(Color::Rgb(30, 30, 40)));

    frame.render_widget(paragraph, area);
}

fn key_hint<'a>(key: &'a str, desc: &'a str) -> Span<'a> {
    Span::styled(
        format!("[{key}] {desc}"),
        Style::default().fg(Color::DarkGray),
    )
}

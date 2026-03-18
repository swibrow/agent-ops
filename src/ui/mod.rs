pub mod dashboard;
pub mod details;
pub mod history;
pub mod projects;
pub mod widgets;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Padding, Tabs};

use crate::app::{ActiveView, App};

const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 20;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Terminal size guard
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_too_small(frame, area);
        return;
    }

    // Main layout: header + tabs + content + footer
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // ASCII header
            Constraint::Length(1), // Tab bar
            Constraint::Min(10),   // Content
            Constraint::Length(1), // Help footer / search bar / status
        ])
        .split(area);

    // ASCII header
    widgets::ascii_header::draw(frame, layout[0], app.tick_count);

    // Tab bar
    draw_tab_bar(frame, layout[1], &app.active_view, app.active_pane_count);

    // Content
    match app.active_view {
        ActiveView::Dashboard => dashboard::draw(frame, layout[2], app),
        ActiveView::Projects => projects::draw(frame, layout[2], app),
        ActiveView::History => history::draw(frame, layout[2], app),
    }

    // Footer: search bar, error, or help
    if app.search_active {
        draw_search_bar(frame, layout[3], &app.search_query);
    } else if let Some(ref error) = app.last_error {
        draw_error_bar(frame, layout[3], error);
    } else if let Some(ref msg) = app.status_message {
        draw_status_bar(frame, layout[3], msg);
    } else {
        widgets::help_footer::draw(frame, layout[3], &app.active_view);
    }

    // Overlays
    if app.show_detail {
        details::draw(frame, app);
    }

    if app.show_help {
        draw_help_overlay(frame, area);
    }

    if app.show_quit_confirm {
        draw_quit_confirm(frame, area);
    }
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let msg = format!(
        "Terminal too small ({}\u{00d7}{}). Need at least {MIN_WIDTH}\u{00d7}{MIN_HEIGHT}.",
        area.width, area.height,
    );
    let paragraph = ratatui::widgets::Paragraph::new(msg)
        .style(Style::default().fg(Color::Red))
        .alignment(Alignment::Center);
    let centered = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Percentage(45),
        ])
        .split(area);
    frame.render_widget(paragraph, centered[1]);
}

fn draw_tab_bar(frame: &mut Frame, area: Rect, active: &ActiveView, pane_count: usize) {
    let tab_titles = vec![
        Line::from(" Dashboard "),
        Line::from(" Projects "),
        Line::from(" History "),
    ];

    let selected = match active {
        ActiveView::Dashboard => 0,
        ActiveView::Projects => 1,
        ActiveView::History => 2,
    };

    let agent_count = format!(" {} active agents ", pane_count);

    let tabs_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),
            Constraint::Length(agent_count.len() as u16 + 2),
        ])
        .split(area);

    let tabs = Tabs::new(tab_titles)
        .select(selected)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(Style::default().fg(Color::Cyan).bold())
        .divider("│");

    frame.render_widget(tabs, tabs_layout[0]);

    let count_span = Span::styled(agent_count, Style::default().fg(Color::Green).bold());
    frame.render_widget(count_span, tabs_layout[1]);
}

fn draw_search_bar(frame: &mut Frame, area: Rect, query: &str) {
    let line = Line::from(vec![
        Span::styled(" / ", Style::default().fg(Color::Yellow).bold()),
        Span::styled(query, Style::default().fg(Color::White)),
        Span::styled("█", Style::default().fg(Color::Yellow)), // cursor
    ]);
    let paragraph =
        ratatui::widgets::Paragraph::new(line).style(Style::default().bg(Color::Rgb(40, 40, 55)));
    frame.render_widget(paragraph, area);
}

fn draw_error_bar(frame: &mut Frame, area: Rect, error: &str) {
    let line = Line::from(vec![
        Span::styled(" ✗ ", Style::default().fg(Color::Red).bold()),
        Span::styled(error, Style::default().fg(Color::Red)),
    ]);
    let paragraph =
        ratatui::widgets::Paragraph::new(line).style(Style::default().bg(Color::Rgb(50, 20, 20)));
    frame.render_widget(paragraph, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, msg: &str) {
    let line = Line::from(vec![
        Span::styled(" ✓ ", Style::default().fg(Color::Green).bold()),
        Span::styled(msg, Style::default().fg(Color::Green)),
    ]);
    let paragraph =
        ratatui::widgets::Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 40, 20)));
    frame.render_widget(paragraph, area);
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 70, area);
    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "KEYBINDINGS",
            Style::default().fg(Color::Cyan).bold(),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  1/2/3  ", Style::default().fg(Color::Yellow)),
            Span::raw("Switch tabs"),
        ]),
        Line::from(vec![
            Span::styled("  Tab    ", Style::default().fg(Color::Yellow)),
            Span::raw("Next tab"),
        ]),
        Line::from(vec![
            Span::styled("  j/k    ", Style::default().fg(Color::Yellow)),
            Span::raw("Navigate up/down"),
        ]),
        Line::from(vec![
            Span::styled("  Enter  ", Style::default().fg(Color::Yellow)),
            Span::raw("Open session details"),
        ]),
        Line::from(vec![
            Span::styled("  r      ", Style::default().fg(Color::Yellow)),
            Span::raw("Resume session"),
        ]),
        Line::from(vec![
            Span::styled("  s      ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle sort (Projects)"),
        ]),
        Line::from(vec![
            Span::styled("  f/F    ", Style::default().fg(Color::Yellow)),
            Span::raw("Filter / Clear filter"),
        ]),
        Line::from(vec![
            Span::styled("  /      ", Style::default().fg(Color::Yellow)),
            Span::raw("Search"),
        ]),
        Line::from(vec![
            Span::styled("  e      ", Style::default().fg(Color::Yellow)),
            Span::raw("Open in editor (Projects)"),
        ]),
        Line::from(vec![
            Span::styled("  y      ", Style::default().fg(Color::Yellow)),
            Span::raw("Copy session ID (Detail)"),
        ]),
        Line::from(vec![
            Span::styled("  Space  ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle pane preview"),
        ]),
        Line::from(vec![
            Span::styled("  R      ", Style::default().fg(Color::Yellow)),
            Span::raw("Force refresh"),
        ]),
        Line::from(vec![
            Span::styled("  ?      ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle this help"),
        ]),
        Line::from(vec![
            Span::styled("  q      ", Style::default().fg(Color::Yellow)),
            Span::raw("Quit"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray).italic(),
        )),
    ];

    let help = ratatui::widgets::Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Help ")
            .title_style(Style::default().fg(Color::Cyan).bold())
            .padding(Padding::new(2, 2, 1, 1)),
    );

    frame.render_widget(help, popup_area);
}

fn draw_quit_confirm(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(35, 20, area);
    frame.render_widget(Clear, popup);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Quit agent-ops?",
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y/Enter] ", Style::default().fg(Color::Green)),
            Span::styled("Yes", Style::default().fg(Color::White)),
            Span::styled("    [any] ", Style::default().fg(Color::Red)),
            Span::styled("Cancel", Style::default().fg(Color::White)),
        ]),
    ];

    let paragraph = ratatui::widgets::Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .padding(Padding::new(1, 1, 0, 0)),
        );

    frame.render_widget(paragraph, popup);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

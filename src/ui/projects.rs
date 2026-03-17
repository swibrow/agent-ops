use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Gauge, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::App;
use crate::model::project::StalenessLevel;
use crate::ui::widgets::{activity_spark, time};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Project list
            Constraint::Length(3), // Gauge bar
        ])
        .split(area);

    draw_project_list(frame, layout[0], app);
    draw_gauge(frame, layout[1], app);
}

fn draw_project_list(frame: &mut Frame, area: Rect, app: &App) {
    let sort_label = app.project_sort.label();
    let filter_label = if app.project_filter_forgotten {
        " [FORGOTTEN ONLY]"
    } else {
        ""
    };
    let title = format!(" Projects  Sort: [{sort_label}]{filter_label} ");

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title)
        .title_style(Style::default().fg(Color::Cyan).bold())
        .padding(Padding::new(1, 1, 0, 0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let projects = &app.filtered_projects;

    if projects.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  No projects found",
            Style::default().fg(Color::DarkGray).italic(),
        )));
        frame.render_widget(empty, inner);
        return;
    }

    let visible_height = inner.height as usize;
    let lines_per_project = 3;
    let max_visible = visible_height / lines_per_project;
    let scroll_offset = app.project_scroll;
    let selected = app.project_selected;

    let mut lines: Vec<Line> = Vec::new();

    for (i, project) in projects
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(max_visible)
    {
        let is_selected = i == selected;

        let indicator = project.staleness.indicator();
        let indicator_color = project.staleness.color();
        let name_style = if is_selected {
            Style::default().fg(Color::White).bold()
        } else {
            Style::default().fg(indicator_color)
        };

        let time_ago = time::format_time_ago(project.last_activity);
        let session_info = format!("{} sessions", project.total_sessions);

        let spark = activity_spark::mini_spark(&project.daily_activity, 20);

        let status_badge = if project.is_active {
            Span::styled(" ACTIVE ", Style::default().fg(Color::Green).bold())
        } else if project.staleness == StalenessLevel::Forgotten {
            Span::styled(" FORGOTTEN ", Style::default().fg(Color::Red).bold())
        } else {
            Span::raw("")
        };

        let line1 = Line::from(vec![
            Span::raw(if is_selected { "▸ " } else { "  " }),
            Span::styled(
                format!("{indicator} "),
                Style::default().fg(indicator_color).bold(),
            ),
            Span::styled(format!("{:<20}", project.name), name_style),
            Span::styled(
                format!("{:>10}", time_ago),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("  {:>12}", session_info),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            spark,
            status_badge,
        ]);

        let line2 = Line::from(vec![
            Span::raw("    "),
            Span::styled(&project.path, Style::default().fg(Color::Rgb(60, 60, 80))),
            Span::styled(
                format!("  {} messages", project.total_messages),
                Style::default().fg(Color::Rgb(60, 60, 80)),
            ),
        ]);

        let sep = Line::from(Span::styled(
            "",
            Style::default().fg(Color::Rgb(30, 30, 40)),
        ));

        lines.push(line1);
        lines.push(line2);
        lines.push(sep);
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    if projects.len() > max_visible {
        let mut scrollbar_state = ScrollbarState::new(projects.len()).position(scroll_offset);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn draw_gauge(frame: &mut Frame, area: Rect, app: &App) {
    let total = app.total_project_count;
    let active = app.active_project_count;
    let ratio = if total > 0 {
        active as f64 / total as f64
    } else {
        0.0
    };

    let label = format!("{active}/{total} projects active");

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 40)))
        .ratio(ratio)
        .label(Span::styled(
            label,
            Style::default().fg(Color::White).bold(),
        ));

    frame.render_widget(gauge, area);
}

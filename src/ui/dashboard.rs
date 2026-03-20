use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::App;
use crate::ui::widgets::{activity_spark, status_badge, time};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Stats bar
            Constraint::Length(3), // Sparkline
            Constraint::Min(5),    // Session list
        ])
        .split(area);

    draw_stats_bar(frame, layout[0], app);
    draw_sparkline_bar(frame, layout[1], app);
    draw_session_list(frame, layout[2], app);
}

fn draw_stats_bar(frame: &mut Frame, area: Rect, app: &App) {
    let total_cpu: f32 = app.active_sessions.iter().map(|s| s.cpu_percent).sum();
    let total_mem: f32 = app.active_sessions.iter().map(|s| s.memory_mb).sum();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(40, 40, 50)))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let line = Line::from(vec![
        Span::styled("Sessions ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", app.active_sessions.len()),
            Style::default().fg(Color::Green).bold(),
        ),
        Span::styled("   Projects ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}/{}", app.active_project_count, app.total_project_count),
            Style::default().fg(Color::Blue).bold(),
        ),
        Span::styled("   Agents CPU ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1}%", total_cpu),
            Style::default().fg(if total_cpu > 100.0 {
                Color::Red
            } else {
                Color::Yellow
            }),
        ),
        Span::styled("   Agents RAM ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.0} MB", total_mem),
            Style::default().fg(if total_mem > 2048.0 {
                Color::Red
            } else {
                Color::Cyan
            }),
        ),
        Span::styled("   Self ", Style::default().fg(Color::Rgb(50, 50, 65))),
        Span::styled(
            format!(
                "{:.1}% / {:.0} MB",
                app.self_stats.cpu_percent, app.self_stats.memory_mb
            ),
            Style::default().fg(Color::Rgb(70, 70, 90)),
        ),
    ]);

    frame.render_widget(Paragraph::new(line), inner);
}

fn draw_sparkline_bar(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Rgb(40, 40, 50)))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let spark_width = inner.width as usize;
    let spark = activity_spark::spark_line(&app.global_daily_activity, spark_width, true);
    frame.render_widget(Paragraph::new(vec![spark]), inner);
}

fn draw_session_list(frame: &mut Frame, area: Rect, app: &App) {
    let title = match &app.agent_filter {
        Some(agent_type) => format!(" LIVE SESSIONS [{}] ", agent_type.label()),
        None => " LIVE SESSIONS ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title)
        .title_style(Style::default().fg(Color::Cyan).bold())
        .padding(Padding::new(1, 1, 0, 0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Apply agent filter
    let filtered: Vec<&crate::model::session::AgentSession>;
    let sessions: &[&crate::model::session::AgentSession] = match &app.agent_filter {
        Some(filter_type) => {
            filtered = app
                .active_sessions
                .iter()
                .filter(|s| &s.agent_type == filter_type)
                .collect();
            &filtered
        }
        None => {
            filtered = app.active_sessions.iter().collect();
            &filtered
        }
    };

    if sessions.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No active agent sessions detected",
                Style::default().fg(Color::DarkGray).italic(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Start a Claude agent in tmux to see it here",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
        frame.render_widget(empty, inner);
        return;
    }

    let visible_height = inner.height as usize;
    let lines_per_session = 3;
    let max_visible = visible_height / lines_per_session;

    let scroll_offset = app.dashboard_scroll;
    let selected = app.dashboard_selected;

    let mut lines: Vec<Line> = Vec::new();
    let full_width = inner.width as usize;

    for (i, session) in sessions
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(max_visible)
    {
        let is_selected = i == selected;

        let agent_icon = status_badge::agent_type_span(&session.agent_type);
        let activity_icon = status_badge::activity_span(&session.activity, app.tick_count);
        let name_style = if is_selected {
            Style::default().fg(Color::White).bold()
        } else {
            Style::default().fg(Color::Cyan)
        };
        let title = session.pane_title.as_deref().unwrap_or("Claude Code");

        let cpu_color = if session.cpu_percent > 50.0 {
            Color::Red
        } else if session.cpu_percent > 10.0 {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        let mem_color = if session.memory_mb > 512.0 {
            Color::Red
        } else if session.memory_mb > 256.0 {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let right_stats = format!("{:5.1}%  {:.0}MB", session.cpu_percent, session.memory_mb);
        let left_prefix = if is_selected { "▸ " } else { "  " };
        // 2 (prefix) + 2 (agent icon + space) + 2 (activity icon + space) + project_name + 2 (gap) + title
        let left_content_len = 2 + 2 + 2 + session.project_name.len() + 2 + title.len();
        let gap = if full_width > left_content_len + right_stats.len() + 2 {
            full_width - left_content_len - right_stats.len() - 2
        } else {
            1
        };

        let line1 = Line::from(vec![
            Span::raw(left_prefix),
            agent_icon,
            activity_icon,
            Span::styled(&session.project_name, name_style),
            Span::styled("  ", Style::default()),
            Span::styled(title, Style::default().fg(Color::White).italic()),
            Span::raw(" ".repeat(gap)),
            Span::styled(
                format!("{:5.1}%", session.cpu_percent),
                Style::default().fg(cpu_color),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:.0}MB", session.memory_mb),
                Style::default().fg(mem_color),
            ),
        ]);

        let activity_lbl = status_badge::activity_label(&session.activity);
        let tmux_loc = session
            .tmux_pane
            .as_ref()
            .map(|p| format!("{}:{} {}", p.session_name, p.window_index, p.pane_id))
            .unwrap_or_else(|| "detached".to_string());

        let time_ago = time::format_time_ago(session.last_activity.unwrap_or(session.started_at));

        let line2 = Line::from(vec![
            Span::raw("    "),
            activity_lbl,
            Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
            Span::styled(tmux_loc, Style::default().fg(Color::DarkGray)),
            Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &session.project_path,
                Style::default().fg(Color::Rgb(60, 60, 80)),
            ),
            Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
            Span::styled(time_ago, Style::default().fg(Color::Yellow)),
        ]);

        let sep_char = "─".repeat(full_width);
        let sep = Line::from(Span::styled(
            sep_char,
            if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Rgb(35, 35, 45))
            },
        ));

        lines.push(line1);
        lines.push(line2);
        lines.push(sep);
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    if sessions.len() > max_visible {
        let mut scrollbar_state = ScrollbarState::new(sessions.len()).position(scroll_offset);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

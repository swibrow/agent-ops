//! Review view — aggregated work-summary across projects in a time window.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

use crate::app::App;
use crate::model::review::{ReviewProject, ReviewRange, ReviewSummary};
use crate::ui::widgets::time::format_time_ago;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Range picker + summary chips
            Constraint::Length(4), // Totals row
            Constraint::Min(5),    // Projects list
        ])
        .split(area);

    draw_range_picker(frame, layout[0], app);
    draw_totals(frame, layout[1], app);
    draw_projects(frame, layout[2], app);
}

fn draw_range_picker(frame: &mut Frame, area: Rect, app: &App) {
    let ranges = [
        ReviewRange::LastHour,
        ReviewRange::LastDay,
        ReviewRange::LastWeek,
        ReviewRange::LastMonth,
    ];
    let mut spans: Vec<Span> = vec![Span::styled(
        " Range: ",
        Style::default().fg(Color::DarkGray),
    )];
    for (i, r) in ranges.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let label = format!(" {} ", r.short_label());
        let style = if *r == app.review_range {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
    }
    spans.push(Span::raw("   "));
    spans.push(Span::styled(
        "[t/T]",
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::raw(" cycle  "));
    spans.push(Span::styled(
        "[Enter]",
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::raw(" expand/collapse  "));
    spans.push(Span::styled(
        "[R]",
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::raw(" refresh"));

    let paragraph = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::ALL).title(" Review "));
    frame.render_widget(paragraph, area);
}

fn draw_totals(frame: &mut Frame, area: Rect, app: &App) {
    let Some(summary) = &app.review_summary else {
        let msg = Paragraph::new("Loading...")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(msg, area);
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    let stat = |label: &str, value: String, color: Color| -> Paragraph {
        let text = vec![
            Line::from(Span::styled(
                label.to_string(),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                value,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
        ];
        Paragraph::new(text).block(Block::default().borders(Borders::ALL))
    };

    let total_tokens = summary.total_input_tokens + summary.total_output_tokens;

    frame.render_widget(
        stat(
            " Range ",
            app.review_range.label().to_string(),
            Color::Cyan,
        ),
        cols[0],
    );
    frame.render_widget(
        stat(
            " Projects ",
            summary.project_count().to_string(),
            Color::Blue,
        ),
        cols[1],
    );
    frame.render_widget(
        stat(
            " Sessions ",
            summary.session_count().to_string(),
            Color::Green,
        ),
        cols[2],
    );
    frame.render_widget(
        stat(
            " Messages ",
            summary.total_messages.to_string(),
            Color::Yellow,
        ),
        cols[3],
    );
    frame.render_widget(
        stat(
            " Tokens (in+out) ",
            format_tokens(total_tokens),
            Color::Magenta,
        ),
        cols[4],
    );
}

fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn draw_projects(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Projects ")
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(summary) = &app.review_summary else {
        return;
    };

    if summary.projects.is_empty() {
        let msg = Paragraph::new(format!(
            "No activity in {}",
            app.review_range.label().to_lowercase()
        ))
        .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let visible_height = inner.height as usize;

    for (idx, project) in summary
        .projects
        .iter()
        .enumerate()
        .skip(app.review_scroll)
    {
        if lines.len() >= visible_height {
            break;
        }
        let is_selected = idx == app.review_selected;
        let is_expanded = app.review_expanded.contains(&project.project_path);

        lines.push(project_header_line(project, is_selected, is_expanded));

        if is_expanded {
            for session in &project.sessions {
                if lines.len() >= visible_height {
                    break;
                }
                let title = session
                    .summary
                    .clone()
                    .or(session.first_prompt.clone())
                    .unwrap_or_else(|| "(no summary)".to_string());
                let title = truncate(&title, inner.width as usize - 6);
                let token_hint = if session.output_tokens > 0 {
                    format!(
                        " · {}↑ {}↓",
                        format_tokens(session.input_tokens),
                        format_tokens(session.output_tokens),
                    )
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("› ", Style::default().fg(Color::DarkGray)),
                    Span::styled(title, Style::default().fg(Color::White)),
                    Span::styled(
                        format!(
                            "   {} msgs{}  · {}",
                            session.message_count,
                            token_hint,
                            format_time_ago(session.last_activity)
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));

                for prompt in &session.key_prompts {
                    if lines.len() >= visible_height {
                        break;
                    }
                    let t = truncate(prompt, inner.width as usize - 10);
                    lines.push(Line::from(vec![
                        Span::raw("        "),
                        Span::styled("· ", Style::default().fg(Color::Cyan)),
                        Span::styled(t, Style::default().fg(Color::Gray)),
                    ]));
                }
            }
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn project_header_line(
    project: &ReviewProject,
    selected: bool,
    expanded: bool,
) -> Line<'static> {
    let chevron = if expanded { "▼" } else { "▶" };
    let branches = if project.branches.is_empty() {
        String::new()
    } else {
        format!(" [{}]", project.branches.join(", "))
    };
    let marker = if selected { "▸ " } else { "  " };
    let marker_style = if selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let name_style = if selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    };

    Line::from(vec![
        Span::styled(marker, marker_style),
        Span::styled(format!("{chevron} "), Style::default().fg(Color::DarkGray)),
        Span::styled(project.project_name.clone(), name_style),
        Span::styled(
            format!(
                "  {} sessions · {} msgs · {}",
                project.session_count,
                project.message_count,
                format_time_ago(project.last_activity)
            ),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(branches, Style::default().fg(Color::Blue)),
    ])
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_chars.saturating_sub(3)).collect::<String>())
    }
}

/// Markdown export for the `y` copy shortcut (optional; not yet wired).
#[allow(dead_code)]
pub fn to_markdown(summary: &ReviewSummary, range: ReviewRange) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Review — {}\n\n", range.label()));
    out.push_str(&format!(
        "**{}** projects · **{}** sessions · **{}** messages\n\n",
        summary.project_count(),
        summary.session_count(),
        summary.total_messages
    ));
    for project in &summary.projects {
        out.push_str(&format!("## {}\n", project.project_name));
        out.push_str(&format!(
            "- {} session(s), {} messages\n",
            project.session_count, project.message_count
        ));
        if !project.branches.is_empty() {
            out.push_str(&format!("- Branches: {}\n", project.branches.join(", ")));
        }
        for session in &project.sessions {
            let title = session
                .summary
                .clone()
                .or(session.first_prompt.clone())
                .unwrap_or_else(|| "(no summary)".to_string());
            out.push_str(&format!("- **{title}**\n"));
            for prompt in &session.key_prompts {
                out.push_str(&format!("  - {prompt}\n"));
            }
        }
        out.push('\n');
    }
    out
}

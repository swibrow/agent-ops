use ratatui::prelude::*;

const HEADER: [&str; 4] = [
    r"  ╔═╗ ╔═╗ ╔═╗ ╔╗╔ ╔╦╗   ╔═╗ ╔═╗ ╔═╗ ",
    r"  ╠═╣ ║ ╦ ║╣  ║║║  ║    ║ ║ ╠═╝ ╚═╗ ",
    r"  ╩ ╩ ╚═╝ ╚═╝ ╝╚╝  ╩    ╚═╝ ╩   ╚═╝ ",
    r"  ─── monitor ∙ track ∙ resume ───────",
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

pub fn draw(frame: &mut Frame, area: Rect, tick: u64) {
    let shift = (tick / 8) as usize; // shift every 8 ticks (~400ms)

    let lines: Vec<Line> = HEADER
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

    let paragraph = ratatui::widgets::Paragraph::new(lines).alignment(Alignment::Center);

    frame.render_widget(paragraph, area);
}

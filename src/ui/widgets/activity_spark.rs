use ratatui::prelude::*;

/// Block characters for sparkline (8 levels)
const SPARK_CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Braille dot patterns for high-res sparkline
const BRAILLE_CHARS: [char; 9] = ['⣀', '⣤', '⣴', '⣶', '⣷', '⣾', '⣿', '⣿', '⣿'];

/// Render a sparkline from data values, fitting into the given width
pub fn spark_line(data: &[u64], width: usize, use_braille: bool) -> Line<'static> {
    if data.is_empty() {
        return Line::from(Span::styled(
            "─".repeat(width),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let max_val = *data.iter().max().unwrap_or(&1).max(&1);
    let chars = if use_braille {
        &BRAILLE_CHARS[..]
    } else {
        &SPARK_CHARS[..]
    };

    // Sample data to fit width
    let data_points: Vec<u64> = if data.len() > width {
        // Downsample
        (0..width)
            .map(|i| {
                let start = i * data.len() / width;
                let end = (i + 1) * data.len() / width;
                data[start..end].iter().sum::<u64>() / (end - start) as u64
            })
            .collect()
    } else {
        // Pad with zeros on the left
        let mut padded = vec![0u64; width - data.len()];
        padded.extend_from_slice(data);
        padded
    };

    let spans: Vec<Span> = data_points
        .iter()
        .map(|&val| {
            let normalized = if max_val > 0 {
                (val as f64 / max_val as f64 * (chars.len() - 1) as f64) as usize
            } else {
                0
            };
            let idx = normalized.min(chars.len() - 1);

            // Color gradient from blue (low) to green (mid) to red (high)
            let color = activity_color(val as f64 / max_val.max(1) as f64);

            Span::styled(chars[idx].to_string(), Style::default().fg(color))
        })
        .collect();

    Line::from(spans)
}

/// Compact sparkline for inline use (projects view)
pub fn mini_spark(data: &[u64], width: usize) -> Span<'static> {
    if data.is_empty() {
        return Span::styled("─".repeat(width), Style::default().fg(Color::DarkGray));
    }

    let max_val = *data.iter().max().unwrap_or(&1).max(&1);

    let data_points: Vec<u64> = if data.len() > width {
        (0..width)
            .map(|i| {
                let start = i * data.len() / width;
                let end = (i + 1) * data.len() / width;
                data[start..end].iter().sum::<u64>() / (end - start).max(1) as u64
            })
            .collect()
    } else {
        let mut padded = vec![0u64; width - data.len()];
        padded.extend_from_slice(data);
        padded
    };

    let text: String = data_points
        .iter()
        .map(|&val| {
            let normalized = if max_val > 0 {
                (val as f64 / max_val as f64 * 7.0) as usize
            } else {
                0
            };
            SPARK_CHARS[normalized.min(7)]
        })
        .collect();

    Span::styled(text, Style::default().fg(Color::Cyan))
}

/// Color based on activity intensity (0.0 = low, 1.0 = high)
fn activity_color(intensity: f64) -> Color {
    if intensity < 0.25 {
        Color::Rgb(59, 130, 246) // blue
    } else if intensity < 0.5 {
        Color::Rgb(6, 182, 212) // cyan
    } else if intensity < 0.75 {
        Color::Rgb(34, 197, 94) // green
    } else {
        Color::Rgb(250, 204, 21) // yellow/gold
    }
}

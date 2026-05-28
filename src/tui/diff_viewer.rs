use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

use super::{syntax_highlight, theme};

/// Status of a diff item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Pending,
    Accepted,
    Rejected,
}

/// A single file diff item to render.
#[derive(Debug, Clone)]
pub struct FileDiffItem {
    pub path: String,
    pub diff: String,
    pub stats: String,
    pub status: DiffStatus,
}

impl FileDiffItem {
    #[must_use]
    pub fn new(path: impl Into<String>, diff: impl Into<String>, stats: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            diff: diff.into(),
            stats: stats.into(),
            status: DiffStatus::Pending,
        }
    }
}

/// Render a list of file diff panels with optional selection and status.
pub fn render_diff_viewer(
    f: &mut Frame,
    area: Rect,
    diffs: &[FileDiffItem],
    selected: Option<usize>,
    diff_scroll: usize,
) {
    if diffs.is_empty() || area.width < 10 || area.height < 3 {
        return;
    }

    let palette = theme::palette();
    let mut all_lines = Vec::new();

    for (idx, item) in diffs.iter().enumerate() {
        if idx > 0 {
            all_lines.push(Line::from(""));
        }

        let is_selected = selected == Some(idx);

        // Status indicator
        let status_icon = match item.status {
            DiffStatus::Pending => "○",
            DiffStatus::Accepted => "✓",
            DiffStatus::Rejected => "✗",
        };
        let status_color = match item.status {
            DiffStatus::Pending => palette.dim,
            DiffStatus::Accepted => palette.success,
            DiffStatus::Rejected => palette.danger,
        };

        let header_style = if is_selected {
            Style::default()
                .fg(palette.warning)
                .bg(palette.surface_alt)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD)
        };

        all_lines.push(Line::from(vec![
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::styled(" ", header_style),
            Span::styled(format!("{} ({})", item.path, item.stats), header_style),
        ]));

        // Diff lines with syntax highlighting attempt
        let lang = syntax_highlight::lang_from_path(&item.path);

        for line in item.diff.lines() {
            let styled_line = if line.starts_with("+++ ") || line.starts_with("--- ") {
                Line::from(Span::styled(
                    truncate(line, area.width as usize),
                    Style::default().fg(palette.dim),
                ))
            } else if line.starts_with("@@") {
                Line::from(Span::styled(
                    truncate(line, area.width as usize),
                    Style::default().fg(palette.warning),
                ))
            } else if line.starts_with('+') {
                // Try to highlight the added code portion
                let hl = try_highlight_diff_line(line, lang, true);
                Line::from(hl)
            } else if line.starts_with('-') {
                let hl = try_highlight_diff_line(line, lang, false);
                Line::from(hl)
            } else if line.starts_with("\\") {
                // "\ No newline at end of file"
                Line::from(Span::styled(
                    truncate(line, area.width as usize),
                    Style::default().fg(palette.dim),
                ))
            } else {
                Line::from(Span::styled(
                    truncate(line, area.width as usize),
                    Style::default().fg(palette.dim),
                ))
            };
            all_lines.push(styled_line);
        }
    }

    // Apply scroll offset
    let visible = area.height as usize;
    let max_start = all_lines.len().saturating_sub(visible);
    let start = diff_scroll.min(max_start);
    if start > 0 {
        all_lines.drain(..start);
    }
    all_lines.truncate(visible);

    let paragraph = Paragraph::new(Text::from(all_lines))
        .style(Style::default().bg(palette.canvas))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

/// Try to syntax-highlight a diff line (with + or - prefix).
fn try_highlight_diff_line(line: &str, lang: Option<&str>, is_add: bool) -> Vec<Span<'static>> {
    let prefix = if is_add { "+" } else { "-" };
    let rest = &line[1..];
    let palette = theme::palette();
    let prefix_color = if is_add {
        palette.success
    } else {
        palette.danger
    };
    let prefix_bg = palette.surface_alt;

    let mut spans = vec![Span::styled(
        prefix,
        Style::default().fg(prefix_color).bg(prefix_bg),
    )];

    if let Some(language) = lang {
        let highlighted = syntax_highlight::highlight_code_block(rest, Some(language));
        if let Some(hl_spans) = highlighted.into_iter().next() {
            spans.extend(hl_spans);
            return spans;
        }
    }

    spans.push(Span::styled(
        rest.to_string(),
        Style::default().fg(prefix_color),
    ));
    spans
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

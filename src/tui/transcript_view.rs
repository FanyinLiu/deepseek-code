use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::deepseek::{MessageVisibility, ProtocolMessage, Role};
use crate::tui::{diff_viewer, plan_tracker, subagent_cards, syntax_highlight, theme, view_blocks};

/// Single continuous terminal transcript.
/// No role headers, no extra blank lines, content speaks for itself.
pub struct TranscriptProps<'a> {
    pub messages: &'a [ProtocolMessage],
    pub pending_user_message: Option<&'a str>,
    pub scroll_offset: usize,
    pub plan_summary: Option<&'a str>,
    pub plan_steps: &'a [plan_tracker::PlanStepItem],
    pub plan_current_step: usize,
    pub plan_total_steps: usize,
    pub plan_warnings: &'a [String],
    pub subagents: &'a [subagent_cards::SubagentCard],
    pub global_elapsed_ms: u64,
    pub diffs: &'a [diff_viewer::FileDiffItem],
    pub selected_diff: Option<usize>,
    pub is_streaming: bool,
    pub stream_buffer: &'a str,
    pub reasoning_buffer: &'a str,
    pub show_reasoning: bool,
}

pub fn render_transcript(f: &mut Frame, area: Rect, props: TranscriptProps<'_>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let palette = theme::palette();

    // ── Messages ──
    for msg in props.messages {
        if msg.visibility == MessageVisibility::AuditOnly {
            continue;
        }
        // One blank line between messages only
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_message(&mut lines, msg, area.width);
    }

    if let Some(message) = props
        .pending_user_message
        .filter(|message| !message.trim().is_empty())
    {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.extend(user_text_lines(message, area.width));
    }

    // ── Streaming response ──
    if props.is_streaming || !props.stream_buffer.is_empty() {
        if !lines.is_empty() && !props.stream_buffer.starts_with('\n') {
            lines.push(Line::from(""));
        }
        render_assistant_content(
            &mut lines,
            props.stream_buffer,
            area.width,
            "",
            palette.text,
        );
    }

    // ── Reasoning (hidden by default, shown if toggled) ──
    if props.show_reasoning && !props.reasoning_buffer.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        for line in props.reasoning_buffer.lines() {
            lines.push(Line::from(vec![
                Span::styled("│ ", transcript_style(palette.muted)),
                Span::styled(line.to_string(), transcript_style(palette.dim)),
            ]));
        }
    }

    // ── Inline Plan ──
    if !props.plan_steps.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_inline_plan(
            &mut lines,
            props.plan_summary,
            props.plan_steps,
            props.plan_current_step,
            props.plan_total_steps,
            props.plan_warnings,
        );
    }

    // ── Inline Subagents ──
    if !props.subagents.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_inline_subagents(&mut lines, props.subagents, props.global_elapsed_ms);
    }

    // ── Inline Diffs ──
    if !props.diffs.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_inline_diffs(&mut lines, props.diffs, props.selected_diff);
    }

    // Chat-style scrollback: offset 0 is pinned to the newest visual lines.
    let lines = wrap_visual_lines(&lines, area.width);
    let visible_height = area.height as usize;
    let max_offset = lines.len().saturating_sub(visible_height);
    let hidden_below = props.scroll_offset.min(max_offset);
    let end = lines.len().saturating_sub(hidden_below);
    let start = end.saturating_sub(visible_height);
    let mut visible: Vec<Line> = Vec::with_capacity(visible_height);
    visible.extend(lines[start..end].iter().cloned());

    let text = Text::from(visible);
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(palette.text).bg(palette.canvas))
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn wrap_visual_lines(lines: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    let max_width = (width as usize).max(1);
    let mut out = Vec::new();

    for line in lines {
        if line.spans.is_empty() {
            out.push(Line::from(""));
            continue;
        }

        let mut current: Vec<Span<'static>> = Vec::new();
        let mut current_width = 0usize;

        for span in &line.spans {
            let mut chunk = String::new();
            let style = span.style;
            for ch in span.content.chars() {
                let ch_width = char_display_width(ch);
                if current_width + ch_width > max_width
                    && (!current.is_empty() || !chunk.is_empty())
                {
                    if !chunk.is_empty() {
                        current.push(Span::styled(std::mem::take(&mut chunk), style));
                    }
                    out.push(Line::from(std::mem::take(&mut current)));
                    current_width = 0;
                }
                chunk.push(ch);
                current_width += ch_width;
                if current_width >= max_width {
                    current.push(Span::styled(std::mem::take(&mut chunk), style));
                    out.push(Line::from(std::mem::take(&mut current)));
                    current_width = 0;
                }
            }
            if !chunk.is_empty() {
                current.push(Span::styled(chunk, style));
            }
        }

        if current.is_empty() {
            out.push(Line::from(""));
        } else {
            out.push(Line::from(current));
        }
    }

    out
}

fn render_message(lines: &mut Vec<Line>, msg: &ProtocolMessage, width: u16) {
    let palette = theme::palette();
    if msg.role == Role::User {
        render_user_message(lines, msg, width);
        return;
    }

    let (prefix, fg) = match msg.role {
        Role::User => unreachable!("user messages are rendered above"),
        Role::Assistant => ("● ", palette.text),
        Role::System => ("! ", palette.dim),
        Role::Tool => ("$ ", palette.accent),
    };

    if msg.role == Role::Tool && !msg.tool_results.is_empty() {
        for result in &msg.tool_results {
            let kind = view_blocks::classify_tool(&result.name);
            let view = view_blocks::ToolCallView {
                name: result.name.clone(),
                status: if result.is_error {
                    view_blocks::ViewStatus::Failed
                } else {
                    view_blocks::ViewStatus::Done
                },
                intent: format!("{kind} result"),
                detail: view_blocks::summarize_tool_result(&result.result),
            };
            lines.extend(view_blocks::render_tool_lines(&view, 88));
        }
        return;
    }

    let content = msg.content.to_string_lossy();
    render_assistant_content(lines, &content, width, prefix, fg);

    if !msg.tool_calls.is_empty() {
        for tc in &msg.tool_calls {
            let kind = view_blocks::classify_tool(&tc.function.name);
            let detail =
                view_blocks::summarize_tool_arguments(&tc.function.name, &tc.function.arguments);
            let view = view_blocks::ToolCallView {
                name: tc.function.name.clone(),
                status: view_blocks::ViewStatus::Running,
                intent: format!("{kind} request"),
                detail,
            };
            lines.extend(render_claude_tool_lines(&view, width));
        }
    }
}

fn render_assistant_content(
    lines: &mut Vec<Line>,
    content: &str,
    width: u16,
    prefix: &str,
    fg: Color,
) {
    let palette = theme::palette();
    let visible_content = sanitize_transcript_visible_text(content);
    let blocks = syntax_highlight::parse_markdown(&visible_content);
    let mut first_line = true;

    for block in blocks {
        match block {
            syntax_highlight::MarkdownBlock::Text(text) => {
                let text_lines: Vec<&str> = text.lines().collect();
                let mut line_index = 0usize;
                while line_index < text_lines.len() {
                    if should_hide_transcript_line(text_lines[line_index]) {
                        line_index += 1;
                        continue;
                    }
                    if let Some(table_end) = markdown_table_end(&text_lines, line_index) {
                        let p = if first_line { prefix } else { "" };
                        render_markdown_table(lines, &text_lines[line_index..table_end], width, p);
                        first_line = false;
                        line_index = table_end;
                        continue;
                    }

                    let line = text_lines[line_index];
                    if is_markdown_rule(line) {
                        lines.push(Line::from(vec![Span::styled(
                            "─".repeat(width.saturating_sub(1) as usize),
                            transcript_style(palette.muted),
                        )]));
                        first_line = false;
                        line_index += 1;
                        continue;
                    }
                    let sanitized_line = sanitize_transcript_visible_text(line);
                    let p = if first_line { prefix } else { "" }.to_string();
                    if let Some(duration) = brewed_duration(&sanitized_line) {
                        lines.push(brewed_line(&p, duration));
                        first_line = false;
                        line_index += 1;
                        continue;
                    }
                    let mut spans = vec![Span::styled(p, transcript_style(fg))];
                    spans.extend(inline_spans(&sanitized_line, fg));
                    lines.push(Line::from(spans));
                    first_line = false;
                    line_index += 1;
                }
            }
            syntax_highlight::MarkdownBlock::Heading(level, text) => {
                let p = if first_line { prefix } else { "" }.to_string();
                let color = match level {
                    1 => palette.accent,
                    2 => palette.warning,
                    _ => palette.secondary,
                };
                lines.push(Line::from(vec![
                    Span::styled(p, transcript_style(fg)),
                    Span::styled(text, transcript_style(color).add_modifier(Modifier::BOLD)),
                ]));
                first_line = false;
            }
            syntax_highlight::MarkdownBlock::BlockQuote(text) => {
                let p = if first_line { prefix } else { "" }.to_string();
                let mut spans = vec![
                    Span::styled(p, transcript_style(fg)),
                    Span::styled("│ ", transcript_style(palette.dim)),
                ];
                spans.extend(inline_spans(&text, palette.secondary));
                lines.push(Line::from(spans));
                first_line = false;
            }
            syntax_highlight::MarkdownBlock::CodeBlock { language: _, code } => {
                first_line = false;
                // No language label: code blocks stay compact in the transcript.
                let highlighted = syntax_highlight::highlight_code_block(&code, None);
                for hl_line in highlighted {
                    let mut spans = vec![Span::styled("  ", transcript_style(palette.text))];
                    if hl_line.is_empty() {
                        spans.push(Span::styled("", transcript_style(palette.text)));
                    } else {
                        spans.extend(hl_line.into_iter().map(|span| {
                            let mut style = span.style.bg(palette.canvas);
                            if style.fg.is_none() {
                                style = style.fg(palette.text);
                            }
                            Span::styled(span.content, style)
                        }));
                    }
                    lines.push(Line::from(spans));
                }
            }
        }
    }
}

fn is_markdown_rule(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3
        && trimmed
            .chars()
            .all(|ch| matches!(ch, '-' | '*' | '_' | ' '))
}

fn brewed_duration(line: &str) -> Option<&str> {
    line.trim().strip_prefix("* Brewed for ")
}

fn brewed_line(prefix: &str, duration: &str) -> Line<'static> {
    let p = theme::palette();
    Line::from(vec![
        Span::styled(prefix.to_string(), transcript_style(p.muted)),
        Span::styled("* ", transcript_style(p.muted)),
        Span::styled(
            "Brewed",
            transcript_style(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" for ", transcript_style(p.dim)),
        Span::styled(duration.to_string(), transcript_style(p.dim)),
    ])
}

fn markdown_table_end(lines: &[&str], start: usize) -> Option<usize> {
    if start + 1 >= lines.len()
        || !is_table_row(lines[start])
        || !is_table_separator(lines[start + 1])
    {
        return None;
    }

    let mut end = start + 2;
    while end < lines.len() && is_table_row(lines[end]) {
        end += 1;
    }
    Some(end)
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && parse_table_cells(trimmed).len() >= 2
}

fn is_table_separator(line: &str) -> bool {
    let cells = parse_table_cells(line);
    cells.len() >= 2
        && cells.iter().all(|cell| {
            let trimmed = cell.trim();
            trimmed.len() >= 3
                && trimmed.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
                && trimmed.chars().any(|ch| ch == '-')
        })
}

fn parse_table_cells(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = line
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    if cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    if cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

fn render_markdown_table(lines: &mut Vec<Line>, table_lines: &[&str], width: u16, prefix: &str) {
    let mut rows: Vec<Vec<String>> = table_lines
        .iter()
        .filter(|line| !is_table_separator(line))
        .map(|line| parse_table_cells(line))
        .filter(|cells| cells.len() >= 2)
        .collect();
    if rows.is_empty() {
        return;
    }

    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(column_count, String::new());
    }

    let prefix_width = display_width(prefix);
    let max_width = (width as usize).saturating_sub(prefix_width).max(24);
    let mut widths = table_column_widths(&rows);
    if column_count > 4 || table_total_width(&widths) > max_width {
        render_markdown_table_cards(lines, &rows, width, prefix);
        return;
    }
    shrink_table_widths(&mut widths, max_width);

    let p = theme::palette();
    let border_style = transcript_style(match theme::active_theme() {
        theme::ThemeMode::Light => Color::Rgb(42, 42, 36),
        theme::ThemeMode::Dark | theme::ThemeMode::HighContrast => p.divider,
    });

    lines.push(table_border_line(
        prefix,
        &widths,
        ('┌', '┬', '┐'),
        border_style,
    ));
    for (row_index, row) in rows.iter().enumerate() {
        lines.push(table_row_line(row, &widths, row_index == 0));
        if row_index == 0 && rows.len() > 1 {
            lines.push(table_border_line(
                "",
                &widths,
                ('├', '┼', '┤'),
                border_style,
            ));
        }
    }
    lines.push(table_border_line(
        "",
        &widths,
        ('└', '┴', '┘'),
        border_style,
    ));
}

fn table_column_widths(rows: &[Vec<String>]) -> Vec<usize> {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![3usize; column_count];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(cell).min(34));
        }
    }
    widths
}

fn shrink_table_widths(widths: &mut [usize], max_width: usize) {
    while table_total_width(widths) > max_width {
        let Some((index, widest)) = widths
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, width)| *width)
        else {
            break;
        };
        if widest <= 4 {
            break;
        }
        widths[index] -= 1;
    }
}

fn table_total_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + widths.len() * 3 + 1
}

fn render_markdown_table_cards(
    lines: &mut Vec<Line>,
    rows: &[Vec<String>],
    width: u16,
    prefix: &str,
) {
    if rows.len() < 2 {
        return;
    }

    let p = theme::palette();
    let headers = &rows[0];
    let max_width = width.saturating_sub(2) as usize;
    lines.push(Line::from(vec![
        Span::styled(prefix.to_string(), transcript_style(p.text)),
        Span::styled(
            "Table",
            transcript_style(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({} rows)", rows.len().saturating_sub(1)),
            transcript_style(p.dim),
        ),
    ]));

    for row in rows.iter().skip(1) {
        let id = row.first().map(String::as_str).unwrap_or("");
        let title = row.get(1).map(String::as_str).unwrap_or("");
        let headline = if title.is_empty() {
            id.to_string()
        } else if id.is_empty() {
            title.to_string()
        } else {
            format!("{id}  {title}")
        };
        lines.push(Line::from(vec![
            Span::styled("• ", transcript_style(p.accent)),
            Span::styled(
                truncate_display_width(&headline, max_width.saturating_sub(2)),
                transcript_style(p.text).add_modifier(Modifier::BOLD),
            ),
        ]));

        for (index, cell) in row.iter().enumerate().skip(2) {
            if cell.trim().is_empty() {
                continue;
            }
            let label = headers
                .get(index)
                .map(String::as_str)
                .filter(|header| !header.trim().is_empty())
                .unwrap_or("detail");
            let line = format!("{label}: {}", cell.trim());
            lines.push(Line::from(vec![
                Span::styled("  └ ", transcript_style(p.divider)),
                Span::styled(
                    truncate_display_width(&line, max_width.saturating_sub(4)),
                    transcript_style(p.dim),
                ),
            ]));
        }
    }
}

fn table_border_line(
    prefix: &str,
    widths: &[usize],
    chars: (char, char, char),
    style: Style,
) -> Line<'static> {
    let mut text = String::new();
    text.push(chars.0);
    for (index, width) in widths.iter().enumerate() {
        text.push_str(&"─".repeat(width + 2));
        text.push(if index + 1 == widths.len() {
            chars.2
        } else {
            chars.1
        });
    }
    Line::from(vec![
        Span::styled(prefix.to_string(), transcript_style(theme::palette().text)),
        Span::styled(text, style),
    ])
}

fn table_row_line(row: &[String], widths: &[usize], is_header: bool) -> Line<'static> {
    let p = theme::palette();
    let border_style = transcript_style(match theme::active_theme() {
        theme::ThemeMode::Light => Color::Rgb(42, 42, 36),
        theme::ThemeMode::Dark | theme::ThemeMode::HighContrast => p.divider,
    });
    let mut spans = vec![Span::styled("│", border_style)];
    for (cell, width) in row.iter().zip(widths.iter().copied()) {
        spans.push(Span::styled(" ", transcript_style(p.text)));
        spans.extend(table_cell_spans(cell, width, is_header));
        spans.push(Span::styled(" ", transcript_style(p.text)));
        spans.push(Span::styled("│", border_style));
    }
    Line::from(spans)
}

fn table_cell_spans(cell: &str, width: usize, is_header: bool) -> Vec<Span<'static>> {
    let p = theme::palette();
    let value = truncate_display_width(cell, width);
    let pad = width.saturating_sub(display_width(&value));
    let mut style = transcript_style(p.text);
    if is_header {
        style = style.add_modifier(Modifier::BOLD);
    } else if is_success_status(&value) {
        style = transcript_style(p.success).add_modifier(Modifier::BOLD);
    } else if is_running_status(&value) {
        style = transcript_style(p.accent).add_modifier(Modifier::BOLD);
    } else if is_blocked_status(&value) {
        style = transcript_style(p.dim);
    }
    let mut spans = inline_spans(&value, style.fg.unwrap_or(p.text));
    if is_header || is_success_status(&value) || is_running_status(&value) {
        for span in &mut spans {
            span.style = span.style.add_modifier(Modifier::BOLD);
        }
    }
    spans.push(Span::styled(" ".repeat(pad), style));
    spans
}

fn is_success_status(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("completed") || lower.contains("done") || lower.contains('✓')
}

fn is_running_status(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("in_progress") || lower.contains("running")
}

fn is_blocked_status(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("blocked") || lower.contains("pending")
}

fn render_user_message(lines: &mut Vec<Line>, msg: &ProtocolMessage, width: u16) {
    let content = msg.content.to_string_lossy();
    render_user_text(lines, &content, width);
}

fn render_user_text(lines: &mut Vec<Line>, content: &str, width: u16) {
    lines.extend(user_text_lines(content, width));
}

fn user_text_lines(content: &str, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line in content.lines() {
        let mut text = format!("▸ {}", line.trim_end());
        let max_width = width.saturating_sub(1) as usize;
        text = truncate_display_width(&text, max_width);
        let fill = max_width.saturating_sub(display_width(&text));
        if fill > 0 {
            text.push_str(&" ".repeat(fill));
        }
        lines.push(Line::from(vec![Span::styled(
            text,
            Style::default()
                .fg(user_bar_fg())
                .bg(user_bar_bg())
                .add_modifier(Modifier::BOLD),
        )]));
    }
    lines
}

fn render_claude_tool_lines(view: &view_blocks::ToolCallView, width: u16) -> Vec<Line<'static>> {
    if view.status == view_blocks::ViewStatus::Running {
        if view.name == "run_command" {
            return connected_tool_lines("Running 1 shell command...", &view.detail, width);
        }
        if view.name == "read_file" {
            return connected_tool_lines("Reading 1 file...", &view.detail, width);
        }
        if view.name == "list_dir" {
            return connected_tool_lines("Listing 1 directory...", &view.detail, width);
        }
        if matches!(
            view.name.as_str(),
            "search_files" | "search_code" | "semantic_search"
        ) {
            return connected_tool_lines("Searching workspace...", &view.detail, width);
        }
        if matches!(view.name.as_str(), "fetch_url" | "web_search") {
            return connected_tool_lines("Fetching...", &view.detail, width);
        }
    }

    view_blocks::render_tool_lines(view, 88)
}

fn connected_tool_lines(title: &str, detail: &str, width: u16) -> Vec<Line<'static>> {
    let p = theme::palette();
    let detail = truncate(detail, width.saturating_sub(8) as usize);
    vec![
        Line::from(vec![
            Span::styled("● ", Style::default().fg(p.muted).bg(p.canvas)),
            Span::styled(
                title.to_string(),
                Style::default()
                    .fg(p.text)
                    .bg(p.canvas)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  └ ", Style::default().fg(p.dim).bg(p.canvas)),
            Span::styled(detail, Style::default().fg(p.dim).bg(p.canvas)),
        ]),
    ]
}

fn should_hide_transcript_line(line: &str) -> bool {
    let trimmed = line.trim();
    let without_bang = trimmed.trim_start_matches('!').trim_start();
    let lower = without_bang.to_ascii_lowercase();
    without_bang.starts_with("[Self-verification]")
        || trimmed.starts_with("[Self-verification skipped")
        || trimmed.contains("No verification available for this project type")
        || lower.starts_with("◇ running tool ")
        || lower.starts_with("◆ done tool ")
        || lower.starts_with("◆ done  tool ")
        || lower.starts_with("◈ running tool ")
        || lower.starts_with("◈ running  tool ")
        || lower.starts_with("intent ")
        || lower.starts_with("detail --- ")
        || lower.starts_with("detail todo.md")
}

fn user_bar_bg() -> Color {
    match theme::active_theme() {
        theme::ThemeMode::Light => Color::Rgb(42, 42, 42),
        theme::ThemeMode::Dark | theme::ThemeMode::HighContrast => theme::palette().surface_alt,
    }
}

fn user_bar_fg() -> Color {
    match theme::active_theme() {
        theme::ThemeMode::Light => Color::Rgb(250, 246, 232),
        theme::ThemeMode::Dark | theme::ThemeMode::HighContrast => theme::palette().text,
    }
}

fn transcript_style(fg: ratatui::style::Color) -> Style {
    Style::default().fg(fg).bg(theme::palette().canvas)
}

fn inline_spans(text: &str, default_fg: Color) -> Vec<Span<'static>> {
    let p = theme::palette();
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;

    while !rest.is_empty() {
        if let Some((token, consumed, style)) = next_inline_token(rest, default_fg) {
            if !plain.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut plain),
                    transcript_style(default_fg),
                ));
            }
            spans.push(Span::styled(token, style.bg(p.canvas)));
            rest = &rest[consumed..];
            continue;
        }

        let ch = rest.chars().next().expect("rest is not empty");
        plain.push(ch);
        rest = &rest[ch.len_utf8()..];
    }

    if !plain.is_empty() {
        spans.push(Span::styled(plain, transcript_style(default_fg)));
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), transcript_style(default_fg)));
    }
    spans
}

fn next_inline_token(input: &str, default_fg: Color) -> Option<(String, usize, Style)> {
    let p = theme::palette();

    if let Some(stripped) = input.strip_prefix("**") {
        if let Some(end) = stripped.find("**") {
            let content = &stripped[..end];
            let consumed = 2 + end + 2;
            if !content.is_empty() {
                return Some((
                    content.to_string(),
                    consumed,
                    transcript_style(default_fg).add_modifier(Modifier::BOLD),
                ));
            }
        }
    }

    if let Some(stripped) = input.strip_prefix('`') {
        if let Some(end) = stripped.find('`') {
            let content = &stripped[..end];
            let consumed = 1 + end + 1;
            if !content.is_empty() {
                return Some((
                    content.to_string(),
                    consumed,
                    transcript_style(p.warning).add_modifier(Modifier::BOLD),
                ));
            }
        }
    }

    if starts_with_url_scheme(input) {
        let (token, consumed) = consume_inline_token(input);
        return Some((
            token,
            consumed,
            transcript_style(p.info).add_modifier(Modifier::UNDERLINED),
        ));
    }

    if input.starts_with('/') && input.chars().nth(1).is_some_and(is_command_char) {
        let (token, consumed) = consume_while(input, |ch| ch == '/' || is_command_char(ch));
        return Some((
            token,
            consumed,
            transcript_style(p.info).add_modifier(Modifier::BOLD),
        ));
    }

    if input.starts_with("--") && input.chars().nth(2).is_some_and(is_command_char) {
        let (token, consumed) = consume_while(input, |ch| ch == '-' || is_command_char(ch));
        return Some((token, consumed, transcript_style(p.warning)));
    }

    if input
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    {
        let (token, consumed) = consume_while(input, |ch| ch.is_ascii_alphanumeric() || ch == '_');
        if is_success_status(&token) {
            return Some((
                token,
                consumed,
                transcript_style(p.success).add_modifier(Modifier::BOLD),
            ));
        }
        if is_running_status(&token) {
            return Some((
                token,
                consumed,
                transcript_style(p.accent).add_modifier(Modifier::BOLD),
            ));
        }
        if is_blocked_status(&token) {
            return Some((token, consumed, transcript_style(p.dim)));
        }
        if is_toolish_identifier(&token) {
            return Some((
                token,
                consumed,
                transcript_style(p.info).add_modifier(Modifier::BOLD),
            ));
        }
    }

    None
}

fn is_toolish_identifier(token: &str) -> bool {
    matches!(
        token,
        "TaskCreate"
            | "TodoWrite"
            | "TaskUpdate"
            | "addBlockedBy"
            | "subject"
            | "description"
            | "activeForm"
            | "status"
            | "in_progress"
            | "completed"
    )
}

fn starts_with_url_scheme(input: &str) -> bool {
    const SCHEMES: [&str; 7] = [
        "http://",
        "https://",
        "file://",
        "chrome://",
        "vscode://",
        "app://",
        "mcp://",
    ];
    SCHEMES.iter().any(|scheme| input.starts_with(scheme))
}

fn consume_inline_token(input: &str) -> (String, usize) {
    consume_while(input, |ch| {
        !ch.is_whitespace()
            && !matches!(
                ch,
                '。' | '，' | '、' | '；' | '：' | '！' | '？' | '）' | ')' | ']' | '}'
            )
    })
}

fn consume_while(input: &str, mut pred: impl FnMut(char) -> bool) -> (String, usize) {
    let mut consumed = 0usize;
    let mut token = String::new();
    for ch in input.chars() {
        if !pred(ch) {
            break;
        }
        consumed += ch.len_utf8();
        token.push(ch);
    }
    (token, consumed)
}

fn is_command_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn render_inline_plan(
    lines: &mut Vec<Line>,
    summary: Option<&str>,
    steps: &[plan_tracker::PlanStepItem],
    current_step: usize,
    total_steps: usize,
    warnings: &[String],
) {
    let completed = steps
        .iter()
        .filter(|step| step.status == plan_tracker::PlanStepStatus::Done)
        .count();
    let failed = steps
        .iter()
        .filter(|step| step.status == plan_tracker::PlanStepStatus::Failed)
        .count();
    let total = total_steps.max(steps.len());
    let open = total.saturating_sub(completed + failed);
    let p = theme::palette();
    let use_chinese = plan_uses_chinese(summary, steps, warnings);
    let is_swarm = is_swarm_agent_plan(steps);
    let clean_summary = summary
        .map(clean_plan_summary)
        .filter(|value| !value.trim().is_empty());
    lines.push(Line::from(vec![
        Span::styled("  ", transcript_style(p.text)),
        Span::styled(
            if is_swarm && use_chinese {
                format!("{total} 个 agent")
            } else if is_swarm {
                format!("{total} agents")
            } else if use_chinese {
                format!("{total} 项任务")
            } else {
                format!("{total} tasks")
            },
            transcript_style(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese {
                format!("（{completed} 完成，{open} 未完成）")
            } else {
                format!(" ({completed} done, {open} open)")
            },
            transcript_style(p.dim),
        ),
        Span::styled(
            clean_summary
                .as_deref()
                .filter(|_| is_swarm)
                .map(|value| format!(" · {}", truncate(value, 64)))
                .unwrap_or_default(),
            transcript_style(p.secondary),
        ),
    ]));

    if let Some(s) = clean_summary.as_deref().filter(|_| !is_swarm) {
        let summary_status = aggregate_plan_status(steps);
        let summary_color = plan_color(summary_status);
        lines.push(Line::from(vec![
            Span::styled("  └ ", transcript_style(p.divider)),
            Span::styled(
                format!("{} ", plan_marker(summary_status)),
                transcript_style(summary_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(truncate(s, 84), transcript_style(p.text)),
        ]));
    }

    for warning in warnings {
        lines.push(Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(theme::palette().warning)),
            Span::styled(
                warning.clone(),
                Style::default().fg(theme::palette().warning),
            ),
        ]));
    }

    if steps.is_empty() {
        return;
    }

    let focus_index = steps
        .iter()
        .position(|step| step.status == plan_tracker::PlanStepStatus::Running)
        .unwrap_or_else(|| {
            current_step
                .saturating_sub(1)
                .min(steps.len().saturating_sub(1))
        });
    let visible = visible_plan_range(steps.len(), focus_index, 6);

    if visible.start > 0 {
        lines.push(Line::from(vec![
            Span::styled("  │ ", transcript_style(p.divider)),
            Span::styled(
                if use_chinese {
                    format!("… 前面还有 {} 项任务", visible.start)
                } else {
                    format!("… {} earlier tasks", visible.start)
                },
                transcript_style(p.dim),
            ),
        ]));
    }

    for idx in visible.clone() {
        let step = &steps[idx];
        let color = plan_color(step.status);
        let style = match step.status {
            plan_tracker::PlanStepStatus::Running => {
                transcript_style(p.text).add_modifier(Modifier::BOLD)
            }
            plan_tracker::PlanStepStatus::Done => transcript_style(p.muted),
            _ => transcript_style(color),
        };
        lines.push(Line::from(vec![
            Span::styled("  │ ", transcript_style(p.divider)),
            Span::styled(plan_marker(step.status), transcript_style(color)),
            Span::styled(" ", transcript_style(p.divider)),
            Span::styled(
                if is_swarm {
                    String::new()
                } else {
                    format!("{:>2}. ", idx + 1)
                },
                transcript_style(p.dim),
            ),
            Span::styled(truncate(&plan_display_title(&step.description), 72), style),
            Span::styled(plan_duration_suffix(step), transcript_style(p.dim)),
        ]));
    }

    if visible.end < steps.len() {
        lines.push(Line::from(vec![
            Span::styled("  │ ", transcript_style(p.divider)),
            Span::styled(
                if use_chinese {
                    format!("… 后面还有 {} 项任务", steps.len() - visible.end)
                } else {
                    format!("… {} more tasks", steps.len() - visible.end)
                },
                transcript_style(p.dim),
            ),
        ]));
    }
}

fn is_swarm_agent_plan(steps: &[plan_tracker::PlanStepItem]) -> bool {
    !steps.is_empty()
        && steps
            .iter()
            .all(|step| step.description.trim_start().starts_with("agent "))
}

fn clean_plan_summary(summary: &str) -> String {
    let mut value = summary.trim();
    loop {
        let next = value
            .strip_prefix("蜂群计划：")
            .or_else(|| value.strip_prefix("蜂群任务："))
            .or_else(|| value.strip_prefix("蜂群计划"))
            .or_else(|| value.strip_prefix("蜂群任务"))
            .or_else(|| value.strip_prefix("Swarm plan:"))
            .or_else(|| value.strip_prefix("Swarm task:"))
            .or_else(|| value.strip_prefix("Swarm plan"))
            .or_else(|| value.strip_prefix("Swarm task"))
            .map(str::trim);
        match next {
            Some(rest) if rest != value => value = rest,
            _ => break,
        }
    }
    value.to_string()
}

fn plan_uses_chinese(
    summary: Option<&str>,
    steps: &[plan_tracker::PlanStepItem],
    warnings: &[String],
) -> bool {
    summary.is_some_and(contains_cjk)
        || steps.iter().any(|step| contains_cjk(&step.description))
        || warnings.iter().any(|warning| contains_cjk(warning))
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF
                | 0x3400..=0x4DBF
                | 0x20000..=0x2A6DF
                | 0x2A700..=0x2B73F
                | 0x2B740..=0x2B81F
                | 0x2B820..=0x2CEAF
        )
    })
}

fn visible_plan_range(len: usize, focus: usize, max_visible: usize) -> std::ops::Range<usize> {
    if len <= max_visible {
        return 0..len;
    }
    let half_window = max_visible / 2;
    let mut start = focus.saturating_sub(half_window);
    let mut end = (start + max_visible).min(len);
    if end == len {
        start = len.saturating_sub(max_visible);
    } else if end - start < max_visible {
        end = (start + max_visible).min(len);
    }
    start..end
}

fn plan_display_title(description: &str) -> String {
    let mut value = description.trim();
    for prefix in [
        "Read `", "Search `", "Edit `", "Run `", "读取 `", "搜索 `", "修改 `", "运行 `",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            if let Some((inside, _)) = rest.split_once('`') {
                value = inside.trim();
                break;
            }
        }
    }
    if let Some(rest) = value.strip_prefix("Verify — ") {
        value = rest.trim();
    } else if let Some(rest) = value.strip_prefix("Verify - ") {
        value = rest.trim();
    } else if let Some(rest) = value.strip_prefix("验证 - ") {
        value = rest.trim();
    }
    strip_leading_task_number(value).to_string()
}

fn strip_leading_task_number(value: &str) -> &str {
    let trimmed = value.trim_start();
    let digits_len = trimmed
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if digits_len == 0 {
        return trimmed;
    }
    let rest = &trimmed[digits_len..];
    let rest = rest
        .strip_prefix('.')
        .or_else(|| rest.strip_prefix(')'))
        .unwrap_or(rest);
    if rest.len() == trimmed.len() - digits_len {
        return trimmed;
    }
    rest.trim_start()
}

fn plan_duration_suffix(step: &plan_tracker::PlanStepItem) -> String {
    let use_chinese = contains_cjk(&step.description);
    step.elapsed_ms()
        .map(plan_tracker::format_duration_compact)
        .map(|duration| match step.status {
            plan_tracker::PlanStepStatus::Running => format!("  · {duration}"),
            plan_tracker::PlanStepStatus::Done | plan_tracker::PlanStepStatus::Failed => {
                if use_chinese {
                    format!("  · 用时 {duration}")
                } else {
                    format!("  · took {duration}")
                }
            }
            plan_tracker::PlanStepStatus::Pending => String::new(),
        })
        .unwrap_or_default()
}

fn aggregate_plan_status(steps: &[plan_tracker::PlanStepItem]) -> plan_tracker::PlanStepStatus {
    if steps
        .iter()
        .any(|step| step.status == plan_tracker::PlanStepStatus::Failed)
    {
        plan_tracker::PlanStepStatus::Failed
    } else if steps
        .iter()
        .any(|step| step.status == plan_tracker::PlanStepStatus::Running)
    {
        plan_tracker::PlanStepStatus::Running
    } else if !steps.is_empty()
        && steps
            .iter()
            .all(|step| step.status == plan_tracker::PlanStepStatus::Done)
    {
        plan_tracker::PlanStepStatus::Done
    } else {
        plan_tracker::PlanStepStatus::Pending
    }
}

fn render_inline_subagents(
    lines: &mut Vec<Line>,
    cards: &[subagent_cards::SubagentCard],
    _global_elapsed_ms: u64,
) {
    let running = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Running)
        .count();
    lines.push(view_blocks::header_line(
        if running > 0 {
            format!("agents {running}/{}", cards.len())
        } else {
            format!("agents {}", cards.len())
        },
        if running > 0 {
            view_blocks::ViewStatus::Running
        } else {
            view_blocks::ViewStatus::Done
        },
    ));

    for card in cards {
        let status = match card.status {
            subagent_cards::SubagentCardStatus::Running => view_blocks::ViewStatus::Running,
            subagent_cards::SubagentCardStatus::Done => view_blocks::ViewStatus::Done,
            subagent_cards::SubagentCardStatus::Failed => view_blocks::ViewStatus::Failed,
        };
        let meta = format_duration(
            card.duration_ms
                .unwrap_or_else(|| card.start_time.elapsed().as_millis() as u64),
        );
        let display = card
            .summary
            .as_deref()
            .or(card.last_update.as_deref())
            .unwrap_or(&card.description);
        let display = sanitize_agent_visible_summary(display);
        let mut metadata = vec![("time".to_string(), meta)];
        if card.files_read > 0 || card.files_written > 0 {
            metadata.push((
                "files".to_string(),
                format!("read {} wrote {}", card.files_read, card.files_written),
            ));
        }
        if card.token_usage > 0 {
            metadata.push(("tokens".to_string(), card.token_usage.to_string()));
        }
        lines.extend(view_blocks::render_worker_lines(
            &view_blocks::WorkerReportView {
                name: truncate(&card.agent_type, 12),
                status,
                task: card.description.clone(),
                metadata,
                summary: Some(display),
            },
            70,
        ));
        // Show live output lines for running agents (mirrors sidebar cards).
        if card.status == subagent_cards::SubagentCardStatus::Running
            && !card.recent_lines.is_empty()
        {
            let p = theme::palette();
            for recent in &card.recent_lines {
                lines.push(Line::from(vec![
                    Span::styled("  ╎ ", Style::default().fg(p.divider)),
                    Span::styled(truncate(recent, 72), Style::default().fg(p.muted)),
                ]));
            }
        }
    }
}

fn sanitize_transcript_visible_text(value: &str) -> String {
    value
        .replace(
            "建议：提高 subagent.max_turns 后重试。",
            "建议：缩小任务范围后重试。",
        )
        .replace("或提高 subagent.max_turns 后重试", "后重试")
        .replace("提高 subagent.max_turns", "缩小任务范围")
        .replace("增加 subagent.max_turns", "缩小任务范围")
        .replace("subagent.max_turns", "任务范围")
        .replace("max_turns", "任务范围")
        .replace("max_iterations", "重试次数")
        .replace("达到轮次上限", "未形成可用结论")
        .replace("Subagent reached max turns limit", "子任务未形成可用结论")
        .replace(
            "reached max turn limit",
            "did not produce a usable conclusion",
        )
        .replace("parse error", "工具调用格式不完整")
        .replace("EOF while parsing", "工具调用内容被截断")
        .replace("line 1 column", "位置")
        .replace("Subagent error:", "子任务失败：")
        .replace("tool call arguments", "工具调用参数")
}

fn sanitize_agent_visible_summary(value: &str) -> String {
    sanitize_transcript_visible_text(value)
}

fn render_inline_diffs(
    lines: &mut Vec<Line>,
    diffs: &[diff_viewer::FileDiffItem],
    selected_diff: Option<usize>,
) {
    let diffs = aggregate_diff_items(diffs);
    lines.push(Line::from(vec![Span::styled(
        format!("● Changes ({})", diffs.len()),
        Style::default().fg(theme::palette().accent),
    )]));

    for (idx, item) in diffs.iter().enumerate() {
        let is_selected = selected_diff == Some(idx);
        let status_icon = match item.status {
            diff_viewer::DiffStatus::Pending => "○",
            diff_viewer::DiffStatus::Accepted => "✓",
            diff_viewer::DiffStatus::Rejected => "✗",
        };
        let status_color = match item.status {
            diff_viewer::DiffStatus::Pending => theme::palette().dim,
            diff_viewer::DiffStatus::Accepted => theme::palette().success,
            diff_viewer::DiffStatus::Rejected => theme::palette().danger,
        };
        let header_style = if is_selected {
            Style::default()
                .fg(theme::palette().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::palette().text)
        };
        lines.push(Line::from(vec![
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::styled(" ", Style::default()),
            Span::styled(format!("{} ({})", item.path, item.stats), header_style),
        ]));
    }
}

fn aggregate_diff_items(diffs: &[diff_viewer::FileDiffItem]) -> Vec<diff_viewer::FileDiffItem> {
    let mut items: Vec<diff_viewer::FileDiffItem> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    for diff in diffs {
        if let Some(index) = items.iter().position(|item| item.path == diff.path) {
            items[index] = diff.clone();
            counts[index] += 1;
        } else {
            items.push(diff.clone());
            counts.push(1);
        }
    }
    for (item, count) in items.iter_mut().zip(counts) {
        if count > 1 {
            item.stats = format!("{} · {count} updates", item.stats);
        }
    }
    items
}

fn plan_marker(status: plan_tracker::PlanStepStatus) -> &'static str {
    match status {
        plan_tracker::PlanStepStatus::Pending | plan_tracker::PlanStepStatus::Running => "○",
        plan_tracker::PlanStepStatus::Done | plan_tracker::PlanStepStatus::Failed => "●",
    }
}

fn plan_color(status: plan_tracker::PlanStepStatus) -> ratatui::style::Color {
    let palette = theme::palette();
    match status {
        plan_tracker::PlanStepStatus::Pending => palette.muted,
        plan_tracker::PlanStepStatus::Running => palette.accent,
        plan_tracker::PlanStepStatus::Done => palette.success,
        plan_tracker::PlanStepStatus::Failed => palette.danger,
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }
    let target = max_width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let width = char_display_width(ch);
        if used + width > target {
            break;
        }
        used += width;
        out.push(ch);
    }
    out.push('…');
    out
}

fn display_width(value: &str) -> usize {
    value.chars().map(char_display_width).sum()
}

fn char_display_width(ch: char) -> usize {
    let code = ch as u32;
    if ch.is_control() {
        return 0;
    }
    if ch.is_ascii() {
        return 1;
    }
    if matches!(
        code,
        0x1100..=0x115F
            | 0x2329..=0x232A
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
    ) {
        2
    } else {
        1
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{:.1}m", ms as f64 / 60_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::{
        MessageContent, MessageVisibility, ProtocolMessage, Role, ToolCall, ToolCallFunction,
        ToolResultRecord,
    };
    use ratatui::{backend::TestBackend, Terminal};
    use uuid::Uuid;

    #[test]
    fn visual_wrap_splits_long_lines_before_scrollback() {
        let line = Line::from(vec![Span::styled(
            "abcdefghijkl",
            Style::default().fg(theme::palette().text),
        )]);

        let wrapped = wrap_visual_lines(&[line], 5);

        assert_eq!(wrapped.len(), 3);
        assert_eq!(line_text(&wrapped[0]), "abcde");
        assert_eq!(line_text(&wrapped[1]), "fghij");
        assert_eq!(line_text(&wrapped[2]), "kl");
    }

    fn test_message(content: &str) -> ProtocolMessage {
        ProtocolMessage {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            content: MessageContent::from(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: Uuid::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn user_message(content: &str) -> ProtocolMessage {
        ProtocolMessage {
            id: Uuid::new_v4(),
            role: Role::User,
            content: MessageContent::from(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            turn_id: Uuid::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        }
    }

    fn tool_message(name: &str, result: &str) -> ProtocolMessage {
        ProtocolMessage {
            id: Uuid::new_v4(),
            role: Role::Tool,
            content: MessageContent::None,
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_results: vec![ToolResultRecord {
                tool_call_id: "tool-1".to_string(),
                name: name.to_string(),
                result: result.to_string(),
                is_error: false,
            }],
            turn_id: Uuid::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        }
    }

    fn assistant_tool_call(name: &str, arguments: &str) -> ProtocolMessage {
        ProtocolMessage {
            id: Uuid::new_v4(),
            role: Role::Assistant,
            content: MessageContent::None,
            reasoning_content: None,
            tool_calls: vec![ToolCall {
                id: "tool-1".to_string(),
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                },
            }],
            tool_results: Vec::new(),
            turn_id: Uuid::new_v4(),
            sub_turn_id: None,
            visibility: MessageVisibility::UserVisible,
        }
    }

    fn render_text(messages: &[ProtocolMessage], scroll_offset: usize, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, height)).expect("terminal");
        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages,
                        pending_user_message: None,
                        scroll_offset,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn transcript_defaults_to_latest_lines() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = test_message("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

        let rendered = render_text(&[msg], 0, 3);

        assert!(!rendered.contains("line 1"));
        assert!(rendered.contains("line 4"));
        assert!(rendered.contains("line 6"));
    }

    #[test]
    fn transcript_scroll_offset_shows_older_lines() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = test_message("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

        let rendered = render_text(&[msg], 2, 3);

        assert!(rendered.contains("line 2"));
        assert!(rendered.contains("line 4"));
        assert!(!rendered.contains("line 6"));
    }

    #[test]
    fn light_theme_transcript_uses_dark_readable_ink_for_chinese_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 4)).expect("terminal");
        let msg = test_message("你好！我是 DeepSeek-Code。");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[msg],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let cell = terminal.backend().buffer().cell((0, 0)).expect("cell");
        assert_eq!(cell.fg, theme::LIGHT_PALETTE.text);
        assert_eq!(cell.bg, theme::LIGHT_PALETTE.canvas);
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn inline_spans_highlight_links_commands_flags_and_code() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let spans = inline_spans(
            "open chrome://settings/accessibility then `/model` or --model",
            theme::palette().text,
        );

        let link = spans
            .iter()
            .find(|span| span.content.as_ref() == "chrome://settings/accessibility")
            .expect("link span");
        assert_eq!(link.style.fg, Some(theme::palette().info));
        assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));

        let command = spans
            .iter()
            .find(|span| span.content.as_ref() == "/model")
            .expect("command span");
        assert_eq!(command.style.fg, Some(theme::palette().warning));
        assert!(command.style.add_modifier.contains(Modifier::BOLD));

        let flag = spans
            .iter()
            .find(|span| span.content.as_ref() == "--model")
            .expect("flag span");
        assert_eq!(flag.style.fg, Some(theme::palette().warning));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn inline_spans_strip_markup_from_bold_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let spans = inline_spans("This is **important**.", theme::palette().text);
        let bold = spans
            .iter()
            .find(|span| span.content.as_ref() == "important")
            .expect("bold span");

        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        assert!(!spans
            .iter()
            .any(|span| span.content.as_ref().contains("**")));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn inline_spans_highlight_report_keywords() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let spans = inline_spans(
            "TaskCreate uses subject description activeForm and status completed",
            theme::palette().text,
        );

        let task = spans
            .iter()
            .find(|span| span.content.as_ref() == "TaskCreate")
            .expect("tool keyword");
        assert_eq!(task.style.fg, Some(theme::palette().info));
        assert!(task.style.add_modifier.contains(Modifier::BOLD));

        let completed = spans
            .iter()
            .find(|span| span.content.as_ref() == "completed")
            .expect("status keyword");
        assert_eq!(completed.style.fg, Some(theme::palette().success));
        assert!(completed.style.add_modifier.contains(Modifier::BOLD));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn self_verification_noise_is_hidden_from_transcript() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = test_message(
            "Done\n[Self-verification] No verification available for this project type\nNext",
        );

        let rendered = render_text(&[msg], 0, 5);

        assert!(rendered.contains("Done"));
        assert!(rendered.contains("Next"));
        assert!(!rendered.contains("Self-verification"));
        assert!(!rendered.contains("No verification available"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn tool_result_renders_as_one_compact_line() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = tool_message("write_file", "--- a/todo.md\n+++ b/todo.md\n@@");

        let rendered = render_text(&[msg], 0, 4);

        assert!(rendered.contains("tool write_file"));
        assert!(rendered.contains("changed todo.md"));
        assert!(!rendered.contains("intent"));
        assert!(!rendered.contains("detail"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn running_read_file_uses_connector_detail_line() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = assistant_tool_call("read_file", r#"{"path":"src/tui/status_bar.rs"}"#);

        let rendered = render_text(&[msg], 0, 4);

        assert!(rendered.contains("Reading 1 file..."));
        assert!(rendered.contains("└ src/tui/status_bar.rs"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn user_message_bar_does_not_wrap_wide_chinese_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(24, 2)).expect("terminal");
        let msg = user_message("继续进行测试");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[msg],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let wrapped_cell = buffer.cell((0, 1)).expect("cell");
        assert_eq!(wrapped_cell.bg, theme::LIGHT_PALETTE.canvas);
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn markdown_table_renders_as_boxed_report() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = test_message(
            "| # | Task | Status |\n|---|---|---|\n| 1 | Build API | completed |\n| 2 | Run tests | in_progress |",
        );

        let rendered = render_text(&[msg], 0, 8);

        assert!(rendered.contains("┌"));
        assert!(rendered.contains("Task"));
        assert!(rendered.contains("Build API"));
        assert!(rendered.contains("completed"));
        assert!(!rendered.contains("|---|"));
        assert!(!rendered.contains("| 1 |"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn markdown_table_status_cells_are_colored() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
        let msg = test_message("| # | Status |\n|---|---|\n| 1 | completed |");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[msg],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let success_cell = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .find(|cell| cell.symbol() == "c")
            .expect("completed text");
        assert_eq!(success_cell.fg, theme::palette().success);
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn markdown_table_pads_cjk_cells_by_display_width() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let spans = table_cell_spans("交互模式", 10, false);
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(display_width(&rendered), 10);
        assert_eq!(char_display_width('│'), 1);
        assert_eq!(char_display_width('交'), 2);
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn wide_markdown_table_renders_as_aligned_cards() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = test_message(
            "| 编号 | 测试模块 | 测试用例 | 前置条件 | 测试步骤 | 预期结果 |\n\
             |---|---|---|---|---|---|\n\
             | TC-01 | 安装/启动 | 验证版本号与帮助信息 | 已安装 CLI | 执行 `todo --version` | 正确输出版本号 |\n\
             | TC-02 | 添加任务 | 添加一条有效任务 | 任务列表为空 | `todo add \"Buy groceries\"` | 提示添加成功 |",
        );

        let rendered = render_text(&[msg], 0, 14);

        assert!(rendered.contains("Table (2 rows)"));
        assert!(rendered.contains("TC-01"));
        assert!(rendered.contains("todo --version"));
        assert!(rendered.contains("Table (2 rows)"));
        assert!(!rendered.contains("┌"));
        assert!(!rendered.contains("| TC-01 |"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn pending_user_message_is_rendered_while_streaming() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).expect("terminal");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: Some("user question should stay visible"),
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        stream_buffer: "answering now",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.contains("user question should stay visible"));
        assert!(rendered.contains("answering now"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn pending_user_message_stays_in_transcript_order() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
        let previous = test_message("上一轮 AI 回答");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[previous],
                        pending_user_message: Some("测试 CLI"),
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        stream_buffer: "AI 正在回答",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        let compact = rendered.replace(' ', "");
        let previous_idx = compact.find("上一轮AI").expect("previous answer");
        let user_idx = compact.find("CLI").expect("pending user");
        let answer_idx = compact.find("AI正在回答").expect("streaming answer");
        assert!(previous_idx < user_idx);
        assert!(user_idx < answer_idx);
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn newest_streaming_output_stays_visible_when_long() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).expect("terminal");
        let long_stream = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7";

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: Some("测试 CLI"),
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        stream_buffer: long_stream,
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(!rendered.contains("CLI"));
        assert!(rendered.contains("line 7"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn streaming_markdown_does_not_show_raw_markers() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).expect("terminal");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        stream_buffer: "## Heading\nThis is **important**.\n---",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.contains("Heading"));
        assert!(rendered.contains("important"));
        assert!(!rendered.contains("##"));
        assert!(!rendered.contains("**"));
        assert!(!rendered.contains("---"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn brewed_line_and_plan_tasks_render_like_compact_report() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
        let steps = vec![
            plan_tracker::PlanStepItem::new("下载前端依赖", plan_tracker::PlanStepStatus::Done)
                .with_duration_ms(1_200),
            plan_tracker::PlanStepItem::new("下载后端依赖", plan_tracker::PlanStepStatus::Done)
                .with_duration_ms(1_400),
            plan_tracker::PlanStepItem::new(
                "拉取 Docker 镜像",
                plan_tracker::PlanStepStatus::Running,
            )
            .with_duration_ms(6_000),
        ];

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: Some("并行构建演示"),
                        plan_steps: &steps,
                        plan_current_step: 2,
                        plan_total_steps: 3,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        stream_buffer: "Done.\n\n* Brewed for 1m 2s",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        let compact = rendered.replace(' ', "");
        assert!(rendered.contains("Brewed for 1m 2s"));
        assert!(compact.contains("3项任务"));
        assert!(compact.contains("2完成"));
        assert!(compact.contains("并行构建演示"));
        assert!(compact.contains("用时1s"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn completed_children_make_summary_line_completed() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
        let steps = vec![
            plan_tracker::PlanStepItem::new("Install deps", plan_tracker::PlanStepStatus::Done),
            plan_tracker::PlanStepItem::new("Run checks", plan_tracker::PlanStepStatus::Done),
        ];

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: Some("Build project"),
                        plan_steps: &steps,
                        plan_current_step: 2,
                        plan_total_steps: 2,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.contains("2 tasks (2 done, 0 open)"));
        assert!(rendered.contains("└ ● Build project"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn agent_summary_hides_internal_turn_budget() {
        let visible = sanitize_agent_visible_summary(
            "子任务未完成：达到轮次上限（10）。建议：提高 subagent.max_turns 后重试。",
        );

        assert!(visible.contains("未形成可用结论"));
        assert!(!visible.contains("轮次"));
        assert!(!visible.contains("max_turns"));
    }

    #[test]
    fn assistant_content_hides_internal_retry_knobs() {
        let visible = sanitize_transcript_visible_text(
            "建议提高 subagent.max_turns 或 max_iterations，避免达到轮次上限。",
        );

        assert!(visible.contains("缩小任务范围"));
        assert!(visible.contains("重试次数"));
        assert!(visible.contains("未形成可用结论"));
        assert!(!visible.contains("subagent.max_turns"));
        assert!(!visible.contains("max_iterations"));
        assert!(!visible.contains("轮次"));
    }

    #[test]
    fn large_inline_plan_is_windowed_numbered_and_clean() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(110, 14)).expect("terminal");
        let steps = (1..=10)
            .map(|idx| {
                let status = if idx == 5 {
                    plan_tracker::PlanStepStatus::Running
                } else {
                    plan_tracker::PlanStepStatus::Pending
                };
                plan_tracker::PlanStepItem::new(
                    format!("Search `{idx}. Inspect subsystem {idx}` — from plan step"),
                    status,
                )
            })
            .collect::<Vec<_>>();

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: None,
                        scroll_offset: 0,
                        plan_summary: Some("Audit project"),
                        plan_steps: &steps,
                        plan_current_step: 5,
                        plan_total_steps: 10,
                        plan_warnings: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.contains("10 tasks (0 done, 10 open)"));
        assert!(rendered.contains("earlier tasks"));
        assert!(rendered.contains("more tasks"));
        assert!(rendered.contains("5. Inspect subsystem 5"));
        assert!(!rendered.contains("Search `"));
        assert!(!rendered.contains("blocked by"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }
}

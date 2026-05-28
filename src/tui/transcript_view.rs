use std::collections::HashMap;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::deepseek::{MessageVisibility, ProtocolMessage, Role, ToolCall};
use crate::tools::todo_state;
use crate::tui::shared_output::{BlockRole, ASSISTANT_PREFIX, SYSTEM_PREFIX, TOOL_LOG_PREFIX};
use crate::tui::{
    diff_viewer, motion, plan_tracker, subagent_cards, syntax_highlight, theme, view_blocks,
};

/// Single continuous terminal transcript.
/// No role headers, no extra blank lines, content speaks for itself.
pub struct TranscriptProps<'a> {
    pub messages: &'a [ProtocolMessage],
    pub pending_user_message: Option<&'a str>,
    pub queued_user_messages: &'a [&'a str],
    pub scroll_offset: usize,
    pub plan_summary: Option<&'a str>,
    pub plan_steps: &'a [plan_tracker::PlanStepItem],
    pub plan_current_step: usize,
    pub plan_total_steps: usize,
    pub plan_warnings: &'a [String],
    pub todo_summary: &'a todo_state::TodoSummary,
    pub todo_items: &'a [todo_state::TodoBoardItem],
    pub subagents: &'a [subagent_cards::SubagentCard],
    pub global_elapsed_ms: u64,
    pub diffs: &'a [diff_viewer::FileDiffItem],
    pub selected_diff: Option<usize>,
    pub is_streaming: bool,
    pub show_streaming_placeholder: bool,
    pub stream_buffer: &'a str,
    pub reasoning_buffer: &'a str,
    pub reasoning_elapsed_ms: u64,
    pub reasoning_tokens: u64,
    pub show_reasoning: bool,
}

const TRANSCRIPT_RENDER_MARGIN_LINES: usize = 120;
const TRANSCRIPT_MAX_WIDTH: u16 = 100;

#[derive(Default)]
struct ToolRenderState {
    run_commands: HashMap<String, String>,
}

impl ToolRenderState {
    fn from_message_window(messages: &[ProtocolMessage], window_start: usize) -> Self {
        let mut state = Self::default();
        let window_start = window_start.min(messages.len());
        let window = &messages[window_start..];
        let mut missing_run_command_ids: Vec<&str> = Vec::new();

        for message in window {
            for tool_call in &message.tool_calls {
                state.record_run_command(tool_call);
            }
            for result in &message.tool_results {
                if result.name == "run_command"
                    && !state.run_commands.contains_key(&result.tool_call_id)
                    && !missing_run_command_ids.contains(&result.tool_call_id.as_str())
                {
                    missing_run_command_ids.push(result.tool_call_id.as_str());
                }
            }
        }

        if !missing_run_command_ids.is_empty() {
            for message in messages[..window_start].iter().rev() {
                for tool_call in &message.tool_calls {
                    if tool_call.function.name != "run_command" {
                        continue;
                    }
                    if let Some(index) = missing_run_command_ids
                        .iter()
                        .position(|id| *id == tool_call.id.as_str())
                    {
                        state.record_run_command(tool_call);
                        missing_run_command_ids.swap_remove(index);
                        if missing_run_command_ids.is_empty() {
                            return state;
                        }
                    }
                }
            }
        }

        state
    }

    fn record_run_command(&mut self, tool_call: &ToolCall) {
        if tool_call.function.name == "run_command" {
            self.run_commands.insert(
                tool_call.id.clone(),
                view_blocks::summarize_tool_arguments(
                    &tool_call.function.name,
                    &tool_call.function.arguments,
                ),
            );
        }
    }

    fn run_command(&self, tool_call_id: &str) -> Option<&str> {
        self.run_commands.get(tool_call_id).map(String::as_str)
    }
}

pub fn render_transcript(f: &mut Frame, area: Rect, props: TranscriptProps<'_>) {
    let content_width = transcript_content_width(area.width);
    let palette = theme::palette();
    let frame = motion::MotionFrame::new(motion::MotionLevel::Subtle, props.global_elapsed_ms);
    let visible_height = area.height as usize;
    let messages = transcript_message_window(
        props.messages,
        content_width,
        visible_height,
        props.scroll_offset,
    );
    let window_start = props.messages.len().saturating_sub(messages.len());
    let mut tool_state = ToolRenderState::from_message_window(props.messages, window_start);
    let mut lines: Vec<Line<'static>> =
        Vec::with_capacity(messages.len().saturating_mul(3).saturating_add(16));

    // ── Messages ──
    for msg in messages {
        if msg.visibility == MessageVisibility::AuditOnly {
            continue;
        }
        // One blank line between messages only
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_message(&mut lines, msg, content_width, &mut tool_state);
    }

    if let Some(message) = props
        .pending_user_message
        .filter(|message| !message.trim().is_empty())
    {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.extend(user_text_lines(message, content_width));
    }

    // ── Streaming response ──
    if !props.stream_buffer.is_empty() || (props.is_streaming && props.show_streaming_placeholder) {
        if !lines.is_empty() && !props.stream_buffer.starts_with('\n') {
            lines.push(Line::from(""));
        }
        if props.stream_buffer.trim().is_empty() && props.is_streaming {
            lines.push(streaming_status_line(frame));
        } else {
            render_assistant_content(
                &mut lines,
                props.stream_buffer,
                content_width,
                "",
                palette.text,
            );
            if props.is_streaming && props.show_streaming_placeholder {
                lines.push(streaming_status_line(frame));
            }
        }
    }

    if !props.queued_user_messages.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_queued_user_messages(&mut lines, props.queued_user_messages, content_width);
    }

    // ── Thinking panel (collapsed by default, expanded with T) ──
    if !props.reasoning_buffer.trim().is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_thinking_panel(
            &mut lines,
            props.reasoning_buffer,
            props.show_reasoning,
            props.reasoning_elapsed_ms,
            props.reasoning_tokens,
        );
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

    // ── Project Todo / Task Board ──
    if !props.todo_items.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_inline_todo_board(
            &mut lines,
            props.todo_summary,
            props.todo_items,
            content_width,
        );
    }

    // ── Inline Subagents ──
    if !props.subagents.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_inline_subagents(
            &mut lines,
            props.subagents,
            props.global_elapsed_ms,
            content_width,
        );
    }

    // ── Inline Diffs ──
    if !props.diffs.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        render_inline_diffs(&mut lines, props.diffs, props.selected_diff);
    }

    let lines = wrap_visual_lines(&lines, content_width);
    let visible = transcript_visible_lines(lines, visible_height, props.scroll_offset);

    let text = Text::from(visible);
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(palette.text).bg(palette.canvas))
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn transcript_message_window(
    messages: &[ProtocolMessage],
    width: u16,
    visible_height: usize,
    scroll_offset: usize,
) -> &[ProtocolMessage] {
    if messages.is_empty() {
        return messages;
    }

    let target_lines = visible_height
        .saturating_add(scroll_offset)
        .saturating_add(TRANSCRIPT_RENDER_MARGIN_LINES)
        .max(visible_height);
    let mut estimated_lines = 0usize;
    let mut start = messages.len();

    for (index, message) in messages.iter().enumerate().rev() {
        start = index;
        estimated_lines =
            estimated_lines.saturating_add(estimate_message_visual_lines(message, width));
        if estimated_lines >= target_lines {
            break;
        }
    }

    &messages[start..]
}

fn estimate_message_visual_lines(message: &ProtocolMessage, width: u16) -> usize {
    if message.visibility == MessageVisibility::AuditOnly {
        return 0;
    }

    let max_width = (width as usize).max(1);
    let mut lines = estimate_text_visual_lines(&message.content.to_string_lossy(), max_width);
    lines = lines.saturating_add(message.tool_calls.iter().map(estimate_tool_lines).sum());
    lines = lines.saturating_add(message.tool_results.iter().map(estimate_result_lines).sum());
    lines.saturating_add(1)
}

fn estimate_tool_lines(tool_call: &ToolCall) -> usize {
    if tool_call.function.name == "run_command" {
        4
    } else {
        3
    }
}

fn estimate_result_lines(result: &crate::deepseek::ToolResultRecord) -> usize {
    if result.name == "run_command" {
        5
    } else {
        3
    }
}

fn estimate_text_visual_lines(text: &str, max_width: usize) -> usize {
    if text.is_empty() {
        return 1;
    }

    let mut count = 0usize;
    for raw_line in text.lines() {
        let width = raw_line.chars().map(char_display_width).sum::<usize>();
        count = count.saturating_add((width / max_width).saturating_add(1));
    }
    count.max(1)
}

fn wrap_visual_lines(lines: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    let max_width = (width as usize).max(1);
    let mut out = Vec::with_capacity(lines.len());

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

fn transcript_visible_lines(
    lines: Vec<Line<'static>>,
    visible_height: usize,
    scroll_offset: usize,
) -> Vec<Line<'static>> {
    if visible_height == 0 {
        return Vec::new();
    }

    let max_offset = lines.len().saturating_sub(visible_height);
    let hidden_below = scroll_offset.min(max_offset);
    let end = lines.len().saturating_sub(hidden_below);
    let start = end.saturating_sub(visible_height);
    lines.into_iter().skip(start).take(end - start).collect()
}

fn render_message(
    lines: &mut Vec<Line<'static>>,
    msg: &ProtocolMessage,
    width: u16,
    tool_state: &mut ToolRenderState,
) {
    let palette = theme::palette();
    if msg.role == Role::User {
        render_user_message(lines, msg, width);
        return;
    }

    let (prefix, fg) = match msg.role {
        Role::User => unreachable!("user messages are rendered above"),
        Role::Assistant => (ASSISTANT_PREFIX, palette.text),
        Role::System => (SYSTEM_PREFIX, palette.dim),
        Role::Tool => (BlockRole::Tool.line_prefix(), palette.accent),
    };

    if msg.role == Role::Tool && !msg.tool_results.is_empty() {
        for result in &msg.tool_results {
            if result.name == "run_command" {
                let command = tool_state.run_command(&result.tool_call_id).unwrap_or("");
                let status = if result.is_error {
                    view_blocks::ViewStatus::Failed
                } else {
                    view_blocks::ViewStatus::Done
                };
                let rendered = render_run_command_lines(command, &result.result, status, width);
                lines.extend(tool_log_lines_from_lines(rendered));
                continue;
            }
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
            lines.extend(tool_log_lines(&view, width));
        }
        return;
    }

    let content = msg.content.to_string_lossy();
    render_assistant_content(lines, &content, width, prefix, fg);

    if !msg.tool_calls.is_empty() {
        for tc in &msg.tool_calls {
            if tc.function.name == "run_command" {
                let command = view_blocks::summarize_tool_arguments(
                    &tc.function.name,
                    &tc.function.arguments,
                );
                tool_state
                    .run_commands
                    .insert(tc.id.clone(), command.clone());
                let rendered =
                    render_run_command_lines(&command, "", view_blocks::ViewStatus::Running, width);
                lines.extend(tool_log_lines_from_lines(rendered));
                continue;
            }
            let kind = view_blocks::classify_tool(&tc.function.name);
            let detail =
                view_blocks::summarize_tool_arguments(&tc.function.name, &tc.function.arguments);
            let view = view_blocks::ToolCallView {
                name: tc.function.name.clone(),
                status: view_blocks::ViewStatus::Running,
                intent: format!("{kind} request"),
                detail,
            };
            lines.extend(render_connected_tool_lines(&view, width));
        }
    }
}

fn render_assistant_content(
    lines: &mut Vec<Line<'static>>,
    content: &str,
    width: u16,
    prefix: &str,
    fg: Color,
) {
    let palette = theme::palette();
    let prefix_fg = if prefix == ASSISTANT_PREFIX {
        palette.assistant
    } else {
        fg
    };
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
                    let mut spans = vec![Span::styled(p, transcript_style(prefix_fg))];
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
                    Span::styled(p, transcript_style(prefix_fg)),
                    Span::styled(text, transcript_style(color).add_modifier(Modifier::BOLD)),
                ]));
                first_line = false;
            }
            syntax_highlight::MarkdownBlock::BlockQuote(text) => {
                let p = if first_line { prefix } else { "" }.to_string();
                let mut spans = vec![
                    Span::styled(p, transcript_style(prefix_fg)),
                    Span::styled("│ ", transcript_style(palette.dim)),
                ];
                spans.extend(inline_spans(&text, palette.secondary));
                lines.push(Line::from(spans));
                first_line = false;
            }
            syntax_highlight::MarkdownBlock::ListItem {
                marker,
                text,
                indent,
            } => {
                let p = if first_line { prefix } else { "" };
                render_markdown_list_item(lines, p, &marker, &text, indent, fg, prefix_fg);
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

fn render_markdown_list_item(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    marker: &str,
    text: &str,
    indent: usize,
    fg: Color,
    prefix_fg: Color,
) {
    let p = theme::palette();
    let (task_marker, text) =
        task_list_marker(text).map_or((None, text), |(marker, rest)| (Some(marker), rest));
    let marker = if let Some(task_marker) = task_marker {
        task_marker.to_string()
    } else if matches!(marker, "-" | "*" | "+") {
        "•".to_string()
    } else {
        marker.to_string()
    };
    let marker_color = if marker == "☑" {
        p.success
    } else if marker == "☐" {
        p.dim
    } else {
        p.warning
    };
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_style(prefix_fg),
    )];
    spans.push(Span::styled(
        " ".repeat(indent.min(8)),
        transcript_style(p.dim),
    ));
    spans.push(Span::styled(
        format!("{marker} "),
        transcript_style(marker_color).add_modifier(Modifier::BOLD),
    ));
    spans.extend(inline_spans(text, fg));
    lines.push(Line::from(spans));
}

fn task_list_marker(text: &str) -> Option<(&'static str, &str)> {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("[ ] ")
        .or_else(|| trimmed.strip_prefix("[ ]\t"))
    {
        return Some(("☐", rest.trim_start()));
    }
    checked_task_list_marker(trimmed)
}

fn checked_task_list_marker(text: &str) -> Option<(&'static str, &str)> {
    let rest = text
        .strip_prefix("[x] ")
        .or_else(|| text.strip_prefix("[X] "))
        .or_else(|| text.strip_prefix("[x]\t"))
        .or_else(|| text.strip_prefix("[X]\t"))?;
    Some(("☑", rest.trim_start()))
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
    let border_style = transcript_style(p.divider);

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
    let border_style = transcript_style(p.divider);
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

fn render_user_message(lines: &mut Vec<Line<'static>>, msg: &ProtocolMessage, width: u16) {
    let content = msg.content.to_string_lossy();
    render_user_text(lines, &content, width);
}

fn render_user_text(lines: &mut Vec<Line<'static>>, content: &str, width: u16) {
    lines.extend(user_text_lines(content, width));
}

fn user_text_lines(content: &str, width: u16) -> Vec<Line<'static>> {
    let p = theme::palette();
    let mut lines = Vec::new();
    for line in content.lines() {
        let max_width = width.saturating_sub(3) as usize;
        let text = truncate_display_width(line.trim_end(), max_width);
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(p.accent).bg(p.canvas)),
            Span::styled(text, Style::default().fg(p.text).bg(p.canvas)),
        ]));
    }
    lines
}

fn render_queued_user_messages(lines: &mut Vec<Line<'static>>, messages: &[&str], width: u16) {
    let p = theme::palette();
    let use_chinese = messages.iter().any(|message| contains_cjk(message));
    let count = messages.len();
    let title = if use_chinese {
        if count == 1 {
            "已排队：当前任务完成后自动发送".to_string()
        } else {
            format!("已排队 {count} 条：将按顺序自动发送")
        }
    } else if count == 1 {
        "Queued: will send after the current task finishes".to_string()
    } else {
        format!("Queued {count}: will send in order after the current task finishes")
    };
    lines.push(Line::from(vec![
        Span::styled("↳ ", transcript_style(p.accent)),
        Span::styled(title, transcript_style(p.dim).add_modifier(Modifier::BOLD)),
    ]));

    let max_width = width.saturating_sub(6) as usize;
    for (idx, message) in messages.iter().take(3).enumerate() {
        let prefix = if count == 1 {
            "  > ".to_string()
        } else {
            format!("  {}. ", idx + 1)
        };
        let text = truncate_display_width(message.trim(), max_width);
        lines.push(Line::from(vec![
            Span::styled(prefix, transcript_style(p.muted)),
            Span::styled(text, transcript_style(p.text)),
        ]));
    }
    if count > 3 {
        let extra = count - 3;
        let label = if use_chinese {
            format!("  ... 还有 {extra} 条")
        } else {
            format!("  ... {extra} more")
        };
        lines.push(Line::from(Span::styled(label, transcript_style(p.dim))));
    }
}

fn render_thinking_panel(
    lines: &mut Vec<Line<'static>>,
    reasoning: &str,
    expanded: bool,
    elapsed_ms: u64,
    tokens: u64,
) {
    let p = theme::palette();
    let state = if expanded { "expanded" } else { "collapsed" };
    let mut metadata = Vec::new();
    if elapsed_ms > 0 {
        metadata.push(format_duration(elapsed_ms));
    }
    if tokens > 0 {
        metadata.push(format!("{tokens} tokens"));
    }
    let suffix = if metadata.is_empty() {
        format!(" · {state} · T toggles")
    } else {
        format!(" · {state} · {} · T toggles", metadata.join(" · "))
    };

    lines.push(Line::from(vec![
        Span::styled(
            "Thinking",
            transcript_style(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(suffix, transcript_style(p.dim)),
    ]));

    if !expanded {
        return;
    }

    let visible_lines = reasoning
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(12);
    for line in visible_lines {
        lines.push(Line::from(vec![
            Span::styled("  │ ", transcript_style(p.divider)),
            Span::styled(line.trim().to_string(), transcript_style(p.dim)),
        ]));
    }
}

fn streaming_status_line(frame: motion::MotionFrame) -> Line<'static> {
    let p = theme::palette();
    Line::from(vec![Span::styled(
        format!("Thinking{}", frame.dots()),
        Style::default().fg(p.dim).bg(p.canvas),
    )])
}

#[derive(Debug, Clone)]
struct CommandProgressLine {
    percent: Option<u8>,
    suffix: String,
    raw: String,
}

fn render_run_command_lines(
    command: &str,
    output: &str,
    status: view_blocks::ViewStatus,
    width: u16,
) -> Vec<Line<'static>> {
    let p = theme::palette();
    let max_width = (width as usize).max(1);
    let command = if command.trim().is_empty() {
        "run_command"
    } else {
        command.trim()
    };
    let command = truncate_display_width(command, max_width.saturating_sub(8).max(12));
    let mut lines = vec![Line::from({
        let mut spans = vec![Span::styled(
            "Execute ",
            transcript_style(p.secondary).add_modifier(Modifier::BOLD),
        )];
        spans.extend(command_inline_spans(&command));
        spans
    })];

    if status == view_blocks::ViewStatus::Running {
        lines.push(command_detail_line("command is running", p.dim));
        lines.push(Line::from(vec![
            Span::styled("  ", transcript_style(p.text)),
            Span::styled(
                "Executing...",
                transcript_style(p.warning).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  (Press Esc to stop)", transcript_style(p.dim)),
        ]));
        return lines;
    }

    let progress_lines = command_progress_lines(output);
    if progress_lines.is_empty() {
        for preview in command_output_preview(output).into_iter().take(2) {
            lines.push(command_detail_line(&preview, p.dim));
        }
    } else {
        for progress in progress_lines {
            lines.push(command_progress_line(&progress, status, max_width));
        }
    }

    lines.push(command_status_line(output, status));
    lines
}

fn command_inline_spans(command: &str) -> Vec<Span<'static>> {
    let p = theme::palette();
    let language = if command.contains("$env:")
        || command.contains("Write-Host")
        || command.contains("Invoke-")
        || command.contains("Test-Path")
    {
        Some("powershell")
    } else {
        Some("bash")
    };
    let highlighted = syntax_highlight::highlight_code_block(command, language);
    let Some(first_line) = highlighted.into_iter().next() else {
        return inline_spans(command, p.text);
    };
    first_line
        .into_iter()
        .map(|span| {
            let mut style = span.style.bg(p.canvas);
            if style.fg.is_none() {
                style = style.fg(p.text);
            }
            Span::styled(span.content, style)
        })
        .collect()
}

fn command_detail_line(value: &str, color: Color) -> Line<'static> {
    let p = theme::palette();
    let mut spans = vec![Span::styled("  ↳ ", transcript_style(p.divider))];
    spans.extend(inline_spans(value, color));
    Line::from(spans)
}

fn command_progress_line(
    progress: &CommandProgressLine,
    status: view_blocks::ViewStatus,
    max_width: usize,
) -> Line<'static> {
    let p = theme::palette();
    let Some(percent) = progress.percent else {
        return command_detail_line(
            &truncate_display_width(&progress.raw, max_width.saturating_sub(4)),
            p.dim,
        );
    };
    if max_width < 24 {
        return command_detail_line(
            &truncate_display_width(
                &format!("{percent}% · {}", progress.suffix),
                max_width.saturating_sub(4),
            ),
            p.dim,
        );
    }

    let percent_text = format!("{percent}%");
    let bar_width = ((max_width as f64 * 0.42) as usize).clamp(8, 34);
    let filled = ((percent as usize * bar_width) + 50) / 100;
    let empty = bar_width.saturating_sub(filled);
    let suffix = truncate_display_width(
        &progress.suffix,
        max_width.saturating_sub(bar_width + percent_text.len() + 11),
    );
    let bar_color = if status == view_blocks::ViewStatus::Done {
        p.success
    } else if status == view_blocks::ViewStatus::Failed {
        p.danger
    } else {
        p.accent
    };
    Line::from(vec![
        Span::styled("  │ ", transcript_style(p.divider)),
        Span::styled("█".repeat(filled), transcript_style(bar_color)),
        Span::styled("░".repeat(empty), transcript_style(p.muted)),
        Span::styled(
            format!(" {percent_text}"),
            transcript_style(bar_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", transcript_style(p.text)),
        Span::styled(suffix, transcript_style(p.dim)),
    ])
}

fn command_status_line(output: &str, status: view_blocks::ViewStatus) -> Line<'static> {
    let p = theme::palette();
    let mut parts = vec![match status {
        view_blocks::ViewStatus::Done => "done".to_string(),
        view_blocks::ViewStatus::Failed => "failed".to_string(),
        view_blocks::ViewStatus::Blocked => "blocked".to_string(),
        view_blocks::ViewStatus::Denied => "denied".to_string(),
        view_blocks::ViewStatus::Cancelled => "cancelled".to_string(),
        view_blocks::ViewStatus::Queued => "queued".to_string(),
        view_blocks::ViewStatus::Running => "running".to_string(),
        view_blocks::ViewStatus::Waiting => "waiting".to_string(),
        view_blocks::ViewStatus::Retrying => "retrying".to_string(),
        view_blocks::ViewStatus::Skipped => "skipped".to_string(),
    }];
    if let Some(exit_code) = command_exit_code(output) {
        parts.push(format!("exit {exit_code}"));
    }
    if let Some(duration_ms) = command_duration_ms(output) {
        parts.push(plan_tracker::format_duration_compact(duration_ms));
    }
    let color = status.color();
    Line::from(vec![
        Span::styled("  └ ", transcript_style(p.divider)),
        Span::styled(
            parts.join(" · "),
            transcript_style(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn command_progress_lines(output: &str) -> Vec<CommandProgressLine> {
    let mut items = Vec::new();
    for line in command_output_lines(output) {
        if !looks_like_progress_line(&line) {
            continue;
        }
        let percent = progress_percent(&line);
        let suffix = percent
            .and_then(|value| progress_suffix(&line, value))
            .unwrap_or_else(|| truncate_display_width(&line, 80));
        items.push(CommandProgressLine {
            percent,
            suffix,
            raw: line,
        });
    }
    let keep_from = items.len().saturating_sub(2);
    items.into_iter().skip(keep_from).collect()
}

fn looks_like_progress_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let has_percent = progress_percent(line).is_some();
    let has_size = [" mib", " mb", " gib", " gb", " kib", " kb", " of "]
        .iter()
        .any(|needle| lower.contains(needle));
    let has_progress_word = lower.contains("download")
        || lower.contains("fetch")
        || lower.contains("install")
        || lower.contains("extract");
    (has_percent && (has_size || has_progress_word || has_progress_bar(line)))
        || (has_progress_bar(line) && has_size)
}

fn has_progress_bar(line: &str) -> bool {
    line.chars()
        .filter(|ch| matches!(ch, '█' | '▓' | '▒' | '░' | '■' | '#' | '='))
        .count()
        >= 4
}

fn progress_percent(line: &str) -> Option<u8> {
    let percent_index = line.find('%')?;
    let before_percent = &line[..percent_index];
    let digits_reversed = before_percent
        .chars()
        .rev()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits_reversed.is_empty() {
        return None;
    }
    let digits = digits_reversed.chars().rev().collect::<String>();
    let value = digits.parse::<u16>().ok()?;
    (value <= 100).then_some(value as u8)
}

fn progress_suffix(line: &str, percent: u8) -> Option<String> {
    let marker = format!("{percent}%");
    let start = line.find(&marker)? + marker.len();
    let suffix = line[start..].trim().trim_matches('|').trim().to_string();
    if suffix.is_empty() {
        None
    } else {
        Some(suffix)
    }
}

fn command_output_preview(output: &str) -> Vec<String> {
    command_output_lines(output)
        .into_iter()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !matches!(lower.as_str(), "stdout:" | "stderr:")
                && !lower.starts_with("exit_code:")
                && !looks_like_progress_line(line)
        })
        .take(2)
        .map(|line| truncate_display_width(&line, 120))
        .collect()
}

fn command_exit_code(output: &str) -> Option<i32> {
    command_output_lines(output).into_iter().find_map(|line| {
        let rest = line.strip_prefix("exit_code:")?.trim();
        let code = rest.split('|').next()?.trim();
        code.parse::<i32>().ok()
    })
}

fn command_duration_ms(output: &str) -> Option<u64> {
    command_output_lines(output).into_iter().find_map(|line| {
        let (_, rest) = line.split_once("duration:")?;
        let digits = rest
            .trim()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        digits.parse::<u64>().ok()
    })
}

fn command_output_lines(output: &str) -> Vec<String> {
    output
        .split(['\n', '\r'])
        .map(strip_ansi_codes)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn strip_ansi_codes(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek().is_some_and(|next| *next == '[') {
            let _ = chars.next();
            for seq in chars.by_ref() {
                if seq.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn render_connected_tool_lines(view: &view_blocks::ToolCallView, width: u16) -> Vec<Line<'static>> {
    tool_log_lines_with_prefix(view, width)
}

fn tool_log_lines(view: &view_blocks::ToolCallView, width: u16) -> Vec<Line<'static>> {
    tool_log_lines_with_prefix(view, width)
}

fn tool_log_lines_with_prefix(view: &view_blocks::ToolCallView, width: u16) -> Vec<Line<'static>> {
    let max_width = width.saturating_sub(4).max(1) as usize;
    let base = view_blocks::render_tool_card_lines(view, max_width);
    base.into_iter().map(tool_log_prefix).collect()
}

fn tool_log_lines_from_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    lines.into_iter().map(tool_log_prefix).collect()
}

fn tool_log_prefix(line: Line<'static>) -> Line<'static> {
    let p = theme::palette();
    let mut spans = Vec::with_capacity(line.spans.len() + 1);
    spans.push(Span::styled(TOOL_LOG_PREFIX, transcript_style(p.dim)));
    spans.extend(line.spans);
    Line::from(spans)
}

fn transcript_content_width(area_width: u16) -> u16 {
    area_width.saturating_sub(2).clamp(1, TRANSCRIPT_MAX_WIDTH)
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

    if let Some(stripped) = input.strip_prefix("***") {
        if let Some(end) = stripped.find("***") {
            let content = &stripped[..end];
            let consumed = 3 + end + 3;
            if !content.is_empty() {
                return Some((
                    content.to_string(),
                    consumed,
                    transcript_style(default_fg)
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
        }
    }

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

    if let Some((token, consumed)) = italic_inline_token(input, '*') {
        return Some((
            token,
            consumed,
            transcript_style(default_fg).add_modifier(Modifier::ITALIC),
        ));
    }

    if let Some((token, consumed)) = italic_inline_token(input, '_') {
        return Some((
            token,
            consumed,
            transcript_style(default_fg).add_modifier(Modifier::ITALIC),
        ));
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
            transcript_style(p.warning).add_modifier(Modifier::BOLD),
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

fn italic_inline_token(input: &str, delimiter: char) -> Option<(String, usize)> {
    let mut chars = input.chars();
    if chars.next()? != delimiter {
        return None;
    }
    if chars.next().is_some_and(|ch| ch == delimiter) {
        return None;
    }
    let stripped = &input[delimiter.len_utf8()..];
    let end = stripped.find(delimiter)?;
    let content = &stripped[..end];
    if content.trim().is_empty() {
        return None;
    }
    Some((
        content.to_string(),
        delimiter.len_utf8() + end + delimiter.len_utf8(),
    ))
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
    let running = steps
        .iter()
        .filter(|step| step.status == plan_tracker::PlanStepStatus::Running)
        .count();
    let total = total_steps.max(steps.len());
    let queued = total.saturating_sub(completed + failed + running);
    let p = theme::palette();
    let use_chinese = plan_uses_chinese(summary, steps, warnings);
    let is_swarm = is_swarm_agent_plan(steps);
    let clean_summary = summary
        .map(clean_plan_summary)
        .filter(|value| !value.trim().is_empty());

    let display_step = if total == 0 {
        0
    } else {
        current_step.saturating_add(1).min(total)
    };
    lines.push(Line::from(vec![
        Span::styled(
            "╭─ ",
            transcript_style(p.divider).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese {
                "任务控制台"
            } else {
                "Mission Control"
            },
            transcript_style(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese {
                format!(" · 计划 {display_step}/{total} · {running} 运行 · {completed} 完成 · {queued} 排队")
            } else {
                format!(" · plan {display_step}/{total} · {running} running · {completed} done · {queued} queued")
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
            Span::styled("│ ", transcript_style(p.divider)),
            Span::styled(
                if use_chinese { "目标  " } else { "goal   " },
                transcript_style(p.muted),
            ),
            Span::styled(
                format!("{} ", plan_marker(summary_status)),
                transcript_style(summary_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(truncate(s, 84), transcript_style(p.text)),
        ]));
    }

    for warning in warnings {
        lines.push(Line::from(vec![
            Span::styled("│ ", transcript_style(p.divider)),
            Span::styled("⚠ ", Style::default().fg(theme::palette().warning)),
            Span::styled(
                warning.clone(),
                Style::default().fg(theme::palette().warning),
            ),
        ]));
    }

    if steps.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "╰─",
            transcript_style(p.divider),
        )]));
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
    let current = &steps[focus_index];
    let current_color = plan_color(current.status);
    lines.push(Line::from(vec![
        Span::styled("│ ", transcript_style(p.divider)),
        Span::styled(
            if use_chinese { "当前  " } else { "now    " },
            transcript_style(p.muted),
        ),
        Span::styled(
            format!("{} ", plan_marker(current.status)),
            transcript_style(current_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            truncate(&plan_display_title(&current.description), 76),
            transcript_style(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(plan_duration_suffix(current), transcript_style(p.dim)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("├─ ", transcript_style(p.divider)),
        Span::styled(
            if is_swarm && use_chinese {
                "Agent 路线"
            } else if is_swarm {
                "Agent lanes"
            } else if use_chinese {
                "执行路线"
            } else {
                "Timeline"
            },
            transcript_style(p.secondary).add_modifier(Modifier::BOLD),
        ),
    ]));

    if visible.start > 0 {
        lines.push(Line::from(vec![
            Span::styled("│   ", transcript_style(p.divider)),
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
            Span::styled("│ ", transcript_style(p.divider)),
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
            Span::styled("│   ", transcript_style(p.divider)),
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
    lines.push(Line::from(vec![Span::styled(
        "╰─",
        transcript_style(p.divider),
    )]));
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

fn render_inline_todo_board(
    lines: &mut Vec<Line>,
    summary: &todo_state::TodoSummary,
    items: &[todo_state::TodoBoardItem],
    width: u16,
) {
    let p = theme::palette();
    let line_width = width.max(32) as usize;
    let mut header = format!(
        "Task Board · {} total · active {} · pending {} · done {}",
        summary.total, summary.in_progress, summary.pending, summary.completed
    );
    if summary.cancelled > 0 {
        header.push_str(&format!(" · cancelled {}", summary.cancelled));
    }
    lines.push(Line::from(vec![
        Span::styled(
            "╭─ ",
            transcript_style(p.divider).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            truncate_display_width(&header, line_width.saturating_sub(3)),
            transcript_style(p.accent).add_modifier(Modifier::BOLD),
        ),
    ]));

    let visible_items = items.iter().take(8);
    for item in visible_items {
        let status = todo_view_status(&item.status);
        let id = truncate_display_width(&item.id, 10);
        let id = format!("{id:<10}");
        let status_label = todo_status_label(&item.status);
        let status_text = format!("{status_label:<9}");
        let priority = if width >= 72 {
            item.priority
                .as_ref()
                .map(|priority| format!(" · {}", truncate_display_width(priority, 12)))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let fixed_width = 2
            + display_width(status.icon())
            + 1
            + display_width(&id)
            + 1
            + display_width(&status_text)
            + display_width(&priority);
        let text_width = line_width.saturating_sub(fixed_width).max(8);
        let text = truncate_display_width(item.display_text(), text_width);
        lines.push(Line::from(vec![
            Span::styled("│ ", transcript_style(p.divider)),
            Span::styled(
                status.icon().to_string(),
                transcript_style(status.color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", transcript_style(p.divider)),
            Span::styled(id, transcript_style(p.secondary)),
            Span::styled(" ", transcript_style(p.divider)),
            Span::styled(status_text, transcript_style(p.muted)),
            Span::styled(text, transcript_style(p.text)),
            Span::styled(priority, transcript_style(p.dim)),
        ]));
    }

    if items.len() > 8 {
        lines.push(Line::from(vec![
            Span::styled("│ ", transcript_style(p.divider)),
            Span::styled(
                format!("+{} more tasks", items.len() - 8),
                transcript_style(p.dim),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("╰─ ", transcript_style(p.divider)),
        Span::styled(".octocode/todos.json", transcript_style(p.dim)),
        Span::styled(" · ", transcript_style(p.divider)),
        Span::styled("/task list", transcript_style(p.warning)),
    ]));
}

fn todo_view_status(status: &str) -> view_blocks::ViewStatus {
    match status {
        "in_progress" => view_blocks::ViewStatus::Running,
        "completed" => view_blocks::ViewStatus::Done,
        "cancelled" | "canceled" => view_blocks::ViewStatus::Cancelled,
        _ => view_blocks::ViewStatus::Queued,
    }
}

fn todo_status_label(status: &str) -> &'static str {
    match status {
        "in_progress" => "active",
        "completed" => "done",
        "cancelled" | "canceled" => "cancelled",
        _ => "pending",
    }
}

fn render_inline_subagents(
    lines: &mut Vec<Line>,
    cards: &[subagent_cards::SubagentCard],
    _global_elapsed_ms: u64,
    width: u16,
) {
    let p = theme::palette();
    let compact = width < 96;
    let line_width = width.max(24) as usize;
    let running = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Running)
        .count();
    let waiting = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::WaitingApproval)
        .count();
    let retrying = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Retrying)
        .count();
    let active = running + waiting + retrying;
    let failed = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Failed)
        .count();
    let blocked = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Blocked)
        .count();
    let cancelled = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Cancelled)
        .count();
    let done = cards
        .iter()
        .filter(|c| c.status == subagent_cards::SubagentCardStatus::Done)
        .count();
    let summaries = cards.iter().filter(|c| c.summary.is_some()).count();
    let files_read: usize = cards.iter().map(|c| c.files_read).sum();
    let files_written: usize = cards.iter().map(|c| c.files_written).sum();
    let token_usage: u64 = cards.iter().map(|c| c.token_usage).sum();
    let mut header = if compact {
        format!("Agents {} · running {running} · done {done}", cards.len())
    } else {
        format!(
            "Agent Team · {} total · {running} running · {done} done · {failed} failed",
            cards.len()
        )
    };
    if waiting > 0 {
        header.push_str(&format!(" · waiting {waiting}"));
    }
    if retrying > 0 {
        header.push_str(&format!(" · retry {retrying}"));
    }
    if blocked > 0 {
        header.push_str(&format!(" · blocked {blocked}"));
    }
    if cancelled > 0 {
        header.push_str(&format!(" · cancelled {cancelled}"));
    }
    let artifacts = if token_usage > 0 {
        format!("{summaries} summaries · files R{files_read} W{files_written} · {token_usage} tok")
    } else {
        format!("{summaries} summaries · files R{files_read} W{files_written}")
    };

    lines.push(Line::from(vec![
        Span::styled(
            "╭─ ",
            transcript_style(p.divider).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            truncate_display_width(&header, line_width.saturating_sub(3)),
            transcript_style(p.accent).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("│ ", transcript_style(p.divider)),
        Span::styled("artifacts ", transcript_style(p.muted)),
        Span::styled(
            truncate_display_width(&artifacts, line_width.saturating_sub(12)),
            transcript_style(if failed > 0 || blocked > 0 {
                p.warning
            } else {
                p.secondary
            }),
        ),
    ]));

    for card in cards {
        let status = card.status.view_status();
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
        let role = truncate(&agent_role_label(&card.agent_type), 14);
        let lane_style = match status {
            view_blocks::ViewStatus::Running => {
                transcript_style(p.text).add_modifier(Modifier::BOLD)
            }
            view_blocks::ViewStatus::Done => transcript_style(p.muted),
            view_blocks::ViewStatus::Failed => transcript_style(p.danger),
            view_blocks::ViewStatus::Blocked => transcript_style(p.danger),
            view_blocks::ViewStatus::Waiting | view_blocks::ViewStatus::Retrying => {
                transcript_style(p.warning).add_modifier(Modifier::BOLD)
            }
            _ => transcript_style(p.text),
        };
        let role_width = if compact { 9 } else { 14 };
        let role = truncate_display_width(&role, role_width);
        let role = format!("{role:<role_width$}");
        let status_label = if compact {
            compact_status_label(status)
        } else {
            status.label()
        };
        let status_width = if compact { 5 } else { 8 };
        let status_text = format!("{status_label:<status_width$}");
        let files = if !compact && (card.files_read > 0 || card.files_written > 0) {
            format!(" · R{} W{}", card.files_read, card.files_written)
        } else {
            String::new()
        };
        let tokens = if !compact && card.token_usage > 0 {
            format!(" · {} tok", card.token_usage)
        } else {
            String::new()
        };
        let meta = format!(" · {meta}{files}{tokens}");
        let fixed_width = 2
            + display_width(status.icon())
            + 1
            + display_width(&role)
            + 1
            + display_width(&status_text)
            + 1
            + display_width(&meta);
        let description_width = line_width.saturating_sub(fixed_width).max(8);
        let description = truncate_display_width(&card.description, description_width);
        lines.push(Line::from(vec![
            Span::styled("│ ", transcript_style(p.divider)),
            Span::styled(
                status.icon().to_string(),
                transcript_style(status.color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", transcript_style(p.divider)),
            Span::styled(role, lane_style),
            Span::styled(" ", transcript_style(p.divider)),
            Span::styled(status_text, transcript_style(p.muted)),
            Span::styled(" ", transcript_style(p.divider)),
            Span::styled(description, lane_style),
            Span::styled(meta, transcript_style(p.dim)),
        ]));
        let detail_label = match card.status {
            subagent_cards::SubagentCardStatus::Running
            | subagent_cards::SubagentCardStatus::Retrying => "now    ",
            subagent_cards::SubagentCardStatus::WaitingApproval => "wait   ",
            subagent_cards::SubagentCardStatus::Blocked => "blocked",
            _ => "output ",
        };
        let detail_width = line_width
            .saturating_sub(2 + 2 + display_width(detail_label))
            .max(8);
        lines.push(Line::from(vec![
            Span::styled("│   ", transcript_style(p.divider)),
            Span::styled(detail_label, transcript_style(p.muted)),
            Span::styled(
                truncate_display_width(&display, detail_width),
                transcript_style(p.secondary),
            ),
        ]));
    }

    if active > 0 {
        lines.push(Line::from(vec![
            Span::styled("╰─ ", transcript_style(p.divider)),
            Span::styled(
                "active agents stay visible here; raw logs stay hidden until needed",
                transcript_style(p.dim),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "╰─",
            transcript_style(p.divider),
        )]));
    }
}

fn agent_role_label(agent_type: &str) -> String {
    match agent_type {
        "code-explorer" => "explorer".to_string(),
        "code-reviewer" => "reviewer".to_string(),
        "planner" => "planner".to_string(),
        "test-runner" => "test-runner".to_string(),
        "worker" => "worker".to_string(),
        other => other.replace('_', "-"),
    }
}

fn compact_status_label(status: view_blocks::ViewStatus) -> &'static str {
    match status {
        view_blocks::ViewStatus::Queued => "queue",
        view_blocks::ViewStatus::Running => "run",
        view_blocks::ViewStatus::Waiting => "wait",
        view_blocks::ViewStatus::Retrying => "retry",
        view_blocks::ViewStatus::Done => "done",
        view_blocks::ViewStatus::Failed => "fail",
        view_blocks::ViewStatus::Blocked => "block",
        view_blocks::ViewStatus::Denied => "deny",
        view_blocks::ViewStatus::Cancelled => "cncl",
        view_blocks::ViewStatus::Skipped => "skip",
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
        plan_tracker::PlanStepStatus::Pending => "□",
        plan_tracker::PlanStepStatus::Running => "○",
        plan_tracker::PlanStepStatus::Done => "✓",
        plan_tracker::PlanStepStatus::Failed => "✗",
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
    UnicodeWidthStr::width(value)
}

fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
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

    #[test]
    fn markdown_lists_render_with_visible_markers() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut lines = Vec::new();
        render_assistant_content(
            &mut lines,
            "- first\n  - nested\n- [x] done\n- [ ] next\n1. second",
            80,
            "● ",
            theme::palette().text,
        );

        let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(rendered.contains("● • first"));
        assert!(rendered.contains("  • nested"));
        assert!(rendered.contains("☑ done"));
        assert!(rendered.contains("☐ next"));
        assert!(rendered.contains("1. second"));
        theme::set_active_theme(theme::ThemeMode::Light);
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
                        queued_user_messages: &[],
                        scroll_offset,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        show_streaming_placeholder: true,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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

    fn render_snapshot(width: u16, height: u16, props: TranscriptProps<'_>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| render_transcript(f, f.area(), props))
            .expect("draw");

        terminal
            .backend()
            .buffer()
            .content()
            .chunks(width as usize)
            .map(|line| {
                line.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
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
    fn narrow_agent_team_compacts_metadata_without_orphan_lines() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut explorer = subagent_cards::SubagentCard::new(
            "019e0c78-8006",
            "code-explorer",
            "Trace plan and agent render paths",
        );
        explorer.status = subagent_cards::SubagentCardStatus::Done;
        explorer.summary = Some("Located transcript and subagent render paths".into());
        explorer.duration_ms = Some(1_250);
        explorer.files_read = 3;

        let mut reviewer = subagent_cards::SubagentCard::new(
            "019e0c78-7fe3",
            "code-reviewer",
            "Review multi-agent UI against 8 competitors",
        );
        reviewer.apply_delta("checking Mission Control density and task visibility");
        reviewer.token_usage = 42;

        let mut planner = subagent_cards::SubagentCard::new(
            "019e0c78-81a4",
            "planner",
            "Plan next UI pass for real swarm runs",
        );
        planner.apply_delta("mapping agent lanes to plan steps");
        planner.files_written = 1;

        let subagents = vec![explorer, reviewer, planner];
        let rendered = render_snapshot(
            80,
            22,
            TranscriptProps {
                messages: &[],
                pending_user_message: None,
                queued_user_messages: &[],
                scroll_offset: 0,
                plan_summary: None,
                plan_steps: &[],
                plan_current_step: 0,
                plan_total_steps: 0,
                plan_warnings: &[],
                todo_summary: &todo_state::TodoSummary::default(),
                todo_items: &[],
                subagents: &subagents,
                global_elapsed_ms: 0,
                diffs: &[],
                selected_diff: None,
                is_streaming: true,
                show_streaming_placeholder: false,
                stream_buffer: "",
                reasoning_buffer: "",
                reasoning_elapsed_ms: 0,
                reasoning_tokens: 0,
                show_reasoning: false,
            },
        );
        let lines: Vec<&str> = rendered.lines().collect();

        assert!(rendered.contains("Agents 3"));
        assert!(rendered.contains("running 2"));
        assert!(rendered.contains("done 1"));
        assert!(rendered.contains("Review multi-agent UI"));
        assert!(rendered.contains("files R3 W1"));
        assert!(rendered.contains("42 tok"));
        assert!(lines.iter().all(|line| line.chars().count() <= 80));
        assert!(!lines
            .iter()
            .any(|line| matches!(line.trim(), "W0" | "W1" | "tok")));
    }

    #[test]
    fn task_board_renders_project_todo_state_as_transcript_block() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let raw_items = vec![
            serde_json::json!({
                "id": "ui",
                "content": "Build task board",
                "active_form": "Building task board",
                "status": "in_progress",
                "priority": "high"
            }),
            serde_json::json!({
                "id": "tests",
                "content": "Add task board tests",
                "status": "pending"
            }),
            serde_json::json!({
                "id": "done",
                "content": "Inspect todo store",
                "status": "completed"
            }),
        ];
        let todo_summary = todo_state::summarize_todo_items(&raw_items);
        let todo_items = todo_state::board_items(&raw_items);

        let rendered = render_snapshot(
            90,
            18,
            TranscriptProps {
                messages: &[],
                pending_user_message: None,
                queued_user_messages: &[],
                scroll_offset: 0,
                plan_summary: None,
                plan_steps: &[],
                plan_current_step: 0,
                plan_total_steps: 0,
                plan_warnings: &[],
                todo_summary: &todo_summary,
                todo_items: &todo_items,
                subagents: &[],
                global_elapsed_ms: 0,
                diffs: &[],
                selected_diff: None,
                is_streaming: false,
                show_streaming_placeholder: false,
                stream_buffer: "",
                reasoning_buffer: "",
                reasoning_elapsed_ms: 0,
                reasoning_tokens: 0,
                show_reasoning: false,
            },
        );

        assert!(rendered.contains("Task Board"));
        assert!(rendered.contains("3 total"));
        assert!(rendered.contains("active 1"));
        assert!(rendered.contains("Building task board"));
        assert!(rendered.contains("Add task board tests"));
        assert!(rendered.contains(".octocode/todos.json"));
        assert!(rendered.contains("/task list"));
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
    fn transcript_message_window_limits_latest_render_work() {
        let messages = (0..900)
            .map(|index| test_message(&format!("message {index}")))
            .collect::<Vec<_>>();

        let window = transcript_message_window(&messages, 80, 10, 0);

        assert!(window.len() < messages.len());
        assert!(window
            .last()
            .unwrap()
            .content
            .to_string_lossy()
            .contains("message 899"));
    }

    #[test]
    fn transcript_message_window_expands_for_deep_scrollback() {
        let messages = (0..120)
            .map(|index| test_message(&format!("message {index}")))
            .collect::<Vec<_>>();

        let window = transcript_message_window(&messages, 80, 10, 10_000);

        assert_eq!(window.len(), messages.len());
    }

    #[test]
    fn tool_state_backfills_run_command_for_windowed_result() {
        let call = assistant_tool_call("run_command", r#"{"command":"cargo test"}"#);
        let result = tool_message("run_command", "ok");
        let newer = test_message("newer message");
        let messages = vec![call, result, newer];

        let state = ToolRenderState::from_message_window(&messages, 1);

        assert!(state
            .run_command("tool-1")
            .expect("run command was backfilled")
            .contains("cargo test"));
    }

    #[test]
    fn transcript_content_width_caps_at_100_with_side_gutter() {
        assert_eq!(transcript_content_width(1), 1);
        assert_eq!(transcript_content_width(80), 78);
        assert_eq!(transcript_content_width(180), 100);
    }

    #[test]
    fn transcript_visible_lines_clip_to_viewport_from_bottom() {
        let lines = (1..=6)
            .map(|index| Line::from(format!("line {index}")))
            .collect::<Vec<_>>();

        let newest = transcript_visible_lines(lines.clone(), 3, 0);
        assert_eq!(newest[0].spans[0].content.as_ref(), "line 4");
        assert_eq!(newest[2].spans[0].content.as_ref(), "line 6");

        let older = transcript_visible_lines(lines, 3, 2);
        assert_eq!(older[0].spans[0].content.as_ref(), "line 2");
        assert_eq!(older[2].spans[0].content.as_ref(), "line 4");
    }

    #[test]
    fn light_theme_transcript_uses_dark_readable_ink_for_chinese_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 4)).expect("terminal");
        let msg = test_message("你好！我是 Octocode。");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[msg],
                        pending_user_message: None,
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        show_streaming_placeholder: true,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
                        show_reasoning: false,
                    },
                );
            })
            .expect("draw");

        // Skip the leading "● " prefix; verify the
        // Chinese body text itself uses the readable ink colour.
        let cell = terminal.backend().buffer().cell((2, 0)).expect("cell");
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
    fn inline_spans_strip_markup_from_italic_and_code_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let spans = inline_spans("Use *care* with `cargo test`.", theme::palette().text);

        let italic = spans
            .iter()
            .find(|span| span.content.as_ref() == "care")
            .expect("italic span");
        assert!(italic.style.add_modifier.contains(Modifier::ITALIC));

        let code = spans
            .iter()
            .find(|span| span.content.as_ref() == "cargo test")
            .expect("code span");
        assert_eq!(code.style.fg, Some(theme::palette().warning));
        assert!(code.style.add_modifier.contains(Modifier::BOLD));
        assert!(!spans.iter().any(|span| {
            let text = span.content.as_ref();
            text.contains('*') || text.contains('`')
        }));
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
    fn running_read_file_uses_view_block_tool_card() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = assistant_tool_call("read_file", r#"{"path":"src/tui/status_bar.rs"}"#);

        let rendered = render_text(&[msg], 0, 4);

        assert!(rendered.contains("running"));
        assert!(rendered.contains("tool read_file"));
        assert!(rendered.contains("└ src/tui/status_bar.rs"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn running_run_command_uses_execute_download_card() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let msg = assistant_tool_call(
            "run_command",
            r#"{"command":"npx playwright install chromium"}"#,
        );

        let rendered = render_text(&[msg], 0, 5);

        assert!(rendered.contains("Execute"));
        assert!(rendered.contains("npx playwright install chromium"));
        assert!(rendered.contains("Executing"));
        assert!(!rendered.contains("tool run_command"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn run_command_result_extracts_recent_download_progress() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let call = assistant_tool_call(
            "run_command",
            r#"{"command":"npx playwright install chromium"}"#,
        );
        let result = tool_message(
            "run_command",
            "stdout:\n|████████░░░░░░░░░░░░| 10% of 181.9 MiB\r|████████████████░░░░| 20% of 181.9 MiB\nexit_code: 0 | duration: 1200ms",
        );

        let rendered = render_text(&[call, result], 0, 8);

        assert!(rendered.contains("Execute"));
        assert!(rendered.contains("npx playwright install chromium"));
        assert!(rendered.contains("20%"));
        assert!(rendered.contains("181.9 MiB"));
        assert!(rendered.contains("done"));
        assert!(!rendered.contains("tool run_command"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn assistant_reasoning_content_is_not_rendered_as_message_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut msg = test_message("visible answer");
        msg.reasoning_content = Some("private chain should stay hidden".to_string());

        let rendered = render_text(&[msg], 0, 4);

        assert!(rendered.contains("visible answer"));
        assert!(!rendered.contains("private chain"));
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        show_streaming_placeholder: true,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        show_streaming_placeholder: true,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: true,
                        stream_buffer: "answering now",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
    fn queued_user_messages_render_as_guidance_while_streaming() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(90, 8)).expect("terminal");
        let queued = ["下一步继续", "再检查审批位置"];

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: Some("当前任务"),
                        queued_user_messages: &queued,
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: true,
                        stream_buffer: "answering now",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
        let compact = rendered.split_whitespace().collect::<String>();
        assert!(compact.contains("已排队2条"));
        assert!(compact.contains("下一步继续"));
        assert!(compact.contains("再检查审批位置"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn streaming_placeholder_can_be_suppressed_by_app_layout() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 4)).expect("terminal");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: Some("测试 CLI"),
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: false,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
        assert!(rendered.replace(' ', "").contains("测试CLI"));
        assert!(!rendered.contains("Thinking"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn thinking_panel_collapsed_shows_metadata_without_reasoning_text() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(90, 6)).expect("terminal");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: Some("修复输入框"),
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: false,
                        stream_buffer: "",
                        reasoning_buffer: "private reasoning should not look like an answer",
                        reasoning_elapsed_ms: 1_500,
                        reasoning_tokens: 12,
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
        assert!(rendered.contains("Thinking"));
        assert!(rendered.contains("collapsed"));
        assert!(rendered.contains("1.5s"));
        assert!(rendered.contains("12 tokens"));
        assert!(!rendered.contains("private reasoning"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }

    #[test]
    fn thinking_panel_expanded_renders_reasoning_inside_panel() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(90, 7)).expect("terminal");

        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    f.area(),
                    TranscriptProps {
                        messages: &[],
                        pending_user_message: Some("修复输入框"),
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: false,
                        stream_buffer: "",
                        reasoning_buffer: "trace layout\nkeep input visible",
                        reasoning_elapsed_ms: 2_000,
                        reasoning_tokens: 9,
                        show_reasoning: true,
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
        assert!(rendered.contains("expanded"));
        assert!(rendered.contains("trace layout"));
        assert!(rendered.contains("keep input visible"));
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: true,
                        stream_buffer: "AI 正在回答",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: true,
                        stream_buffer: long_stream,
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: None,
                        plan_steps: &[],
                        plan_current_step: 0,
                        plan_total_steps: 0,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: true,
                        stream_buffer: "## Heading\nThis is **important**.\n---",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: Some("并行构建演示"),
                        plan_steps: &steps,
                        plan_current_step: 2,
                        plan_total_steps: 3,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: true,
                        show_streaming_placeholder: true,
                        stream_buffer: "Done.\n\n* Brewed for 1m 2s",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
        assert!(compact.contains("任务控制台"));
        assert!(compact.contains("计划3/3"));
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: Some("Build project"),
                        plan_steps: &steps,
                        plan_current_step: 2,
                        plan_total_steps: 2,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        show_streaming_placeholder: true,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
        assert!(rendered.contains("Mission Control"));
        assert!(rendered.contains("plan 2/2"));
        assert!(rendered.contains("2 done"));
        assert!(rendered.contains("✓ Build project"));
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
                        queued_user_messages: &[],
                        scroll_offset: 0,
                        plan_summary: Some("Audit project"),
                        plan_steps: &steps,
                        plan_current_step: 5,
                        plan_total_steps: 10,
                        plan_warnings: &[],
                        todo_summary: &todo_state::TodoSummary::default(),
                        todo_items: &[],
                        subagents: &[],
                        global_elapsed_ms: 0,
                        diffs: &[],
                        selected_diff: None,
                        is_streaming: false,
                        show_streaming_placeholder: true,
                        stream_buffer: "",
                        reasoning_buffer: "",
                        reasoning_elapsed_ms: 0,
                        reasoning_tokens: 0,
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
        assert!(rendered.contains("Mission Control"));
        assert!(rendered.contains("9 queued"));
        assert!(rendered.contains("earlier tasks"));
        assert!(rendered.contains("more tasks"));
        assert!(rendered.contains("5. Inspect subsystem 5"));
        assert!(!rendered.contains("Search `"));
        assert!(!rendered.contains("blocked by"));
        theme::set_active_theme(theme::ThemeMode::Light);
    }
}

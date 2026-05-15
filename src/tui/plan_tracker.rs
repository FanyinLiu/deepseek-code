use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::agent::orchestrator::DecisionKind;

use super::{motion, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStepStatus {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
pub struct PlanStepItem {
    pub description: String,
    pub status: PlanStepStatus,
    pub started_at: Option<std::time::Instant>,
    pub duration_ms: Option<u64>,
}

impl PlanStepItem {
    #[must_use]
    pub fn new(description: impl Into<String>, status: PlanStepStatus) -> Self {
        Self {
            description: description.into(),
            status,
            started_at: (status == PlanStepStatus::Running).then(std::time::Instant::now),
            duration_ms: None,
        }
    }

    #[must_use]
    pub fn elapsed_ms(&self) -> Option<u64> {
        self.duration_ms.or_else(|| {
            self.started_at
                .filter(|_| self.status == PlanStepStatus::Running)
                .map(|started| started.elapsed().as_millis() as u64)
        })
    }

    pub fn transition_to(&mut self, status: PlanStepStatus) {
        if self.status == status {
            return;
        }
        match status {
            PlanStepStatus::Running => {
                self.started_at = Some(std::time::Instant::now());
                self.duration_ms = None;
            }
            PlanStepStatus::Done | PlanStepStatus::Failed => {
                if self.duration_ms.is_none() {
                    self.duration_ms = self
                        .started_at
                        .map(|started| started.elapsed().as_millis() as u64);
                }
            }
            PlanStepStatus::Pending => {
                self.started_at = None;
                self.duration_ms = None;
            }
        }
        self.status = status;
    }

    #[must_use]
    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

#[must_use]
pub fn format_duration_compact(ms: u64) -> String {
    if ms < 1_000 {
        "<1s".to_string()
    } else {
        let seconds = ms / 1_000;
        if seconds < 60 {
            format!("{seconds}s")
        } else {
            let minutes = seconds / 60;
            let rem = seconds % 60;
            if minutes < 60 {
                format!("{minutes}m {rem}s")
            } else {
                let hours = minutes / 60;
                let rem_minutes = minutes % 60;
                let rem_seconds = seconds % 60;
                if rem_seconds == 0 {
                    format!("{hours}h {rem_minutes}m")
                } else {
                    format!("{hours}h {rem_minutes}m {rem_seconds}s")
                }
            }
        }
    }
}

fn duration_label(step: &PlanStepItem) -> String {
    step.elapsed_ms()
        .map(format_duration_compact)
        .map(|duration| match step.status {
            PlanStepStatus::Running => format!(" · {duration}"),
            PlanStepStatus::Done | PlanStepStatus::Failed => format!(" · took {duration}"),
            PlanStepStatus::Pending => String::new(),
        })
        .unwrap_or_default()
}

fn status_icon_with_motion(status: PlanStepStatus, frame: motion::MotionFrame) -> &'static str {
    match status {
        PlanStepStatus::Pending => "○",
        PlanStepStatus::Running if frame.level.is_enabled() => frame.running_icon(),
        PlanStepStatus::Running => "●",
        PlanStepStatus::Done => "✓",
        PlanStepStatus::Failed => "✗",
    }
}

fn status_color(status: PlanStepStatus) -> Color {
    let p = theme::palette();
    match status {
        PlanStepStatus::Pending => p.muted,
        PlanStepStatus::Running => p.accent,
        PlanStepStatus::Done => p.success,
        PlanStepStatus::Failed => p.danger,
    }
}

fn status_label(status: PlanStepStatus) -> &'static str {
    match status {
        PlanStepStatus::Pending => "queued",
        PlanStepStatus::Running => "running",
        PlanStepStatus::Done => "done",
        PlanStepStatus::Failed => "failed",
    }
}

/// Render the plan tracker with optional review warnings shown above the step list.
pub fn render_plan_tracker_with_warnings(
    f: &mut Frame,
    area: Rect,
    summary: &str,
    steps: &[PlanStepItem],
    current_step: usize,
    total_steps: usize,
    warnings: &[String],
) {
    render_plan_tracker_with_warnings_and_motion(
        f,
        area,
        summary,
        steps,
        current_step,
        total_steps,
        warnings,
        motion::MotionFrame::disabled(),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn render_plan_tracker_with_warnings_and_motion(
    f: &mut Frame,
    area: Rect,
    summary: &str,
    steps: &[PlanStepItem],
    current_step: usize,
    total_steps: usize,
    warnings: &[String],
    frame: motion::MotionFrame,
) {
    let display_step = if total_steps == 0 {
        0
    } else {
        current_step.saturating_add(1).min(total_steps)
    };

    if area.width < 3 || area.height < 1 {
        return;
    }

    let mut lines = Vec::new();
    let p = theme::palette();

    // Header: "plan 2/5  summary text"
    let header_summary = if summary.is_empty() {
        String::new()
    } else {
        format!(
            "  {}",
            truncate(summary, area.width.saturating_sub(16) as usize)
        )
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("plan {display_step}/{total_steps}"),
            Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(header_summary, Style::default().fg(p.secondary)),
    ]));

    // Review warnings (if any)
    for warning in warnings {
        let w = truncate(warning, area.width.saturating_sub(4) as usize);
        lines.push(Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(p.warning)),
            Span::styled(w, Style::default().fg(p.warning)),
        ]));
    }

    // Progress bar: ━━━●───  (filled=done, ●=running, ─=pending)
    let bar = progress_bar(steps, total_steps, area.width.saturating_sub(2) as usize);
    if !bar.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            bar,
            Style::default().fg(p.accent),
        )]));
    }

    // Divider
    lines.push(divider_line(area.width));

    let header_height = lines.len();
    let visible = (area.height as usize).saturating_sub(header_height);
    let start = if steps.len() > visible {
        let running_idx = steps
            .iter()
            .position(|s| s.status == PlanStepStatus::Running)
            .unwrap_or(current_step.min(steps.len().saturating_sub(1)));
        running_idx.saturating_sub(visible / 2)
    } else {
        0
    };
    let end = (start + visible).min(steps.len());

    for step in steps.iter().skip(start).take(end - start) {
        let icon = status_icon_with_motion(step.status, frame);
        let color = status_color(step.status);
        let kind = step_kind(&step.description);
        let label = status_label(step.status);
        let duration = duration_label(step);
        let desc_width = area
            .width
            .saturating_sub(23 + duration.chars().count() as u16) as usize;
        let desc = truncate(&step.description, desc_width);

        let (prefix, text_style) = match step.status {
            PlanStepStatus::Running => (
                "▶ ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
            PlanStepStatus::Done => (" ", Style::default().fg(p.muted)),
            PlanStepStatus::Failed => (" ", Style::default().fg(p.danger)),
            PlanStepStatus::Pending => (" ", Style::default().fg(color)),
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(color)),
            Span::styled(icon, Style::default().fg(color)),
            Span::styled(" ", Style::default()),
            Span::styled(format!("{kind:<7}"), Style::default().fg(p.secondary)),
            Span::styled(" ", Style::default()),
            Span::styled(format!("{label:<8}"), Style::default().fg(p.muted)),
            Span::styled(" ", Style::default()),
            Span::styled(desc, text_style),
            Span::styled(duration, Style::default().fg(p.dim)),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(p.canvas))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

/// Render the options selection menu (shown when orchestrator emits OptionsNeeded).
pub fn render_options_panel(
    f: &mut Frame,
    area: Rect,
    kind: DecisionKind,
    title: &str,
    options: &[String],
    selected_index: usize,
) {
    if area.width < 10 || area.height < 3 || options.is_empty() {
        return;
    }

    let p = theme::palette();
    let selected_bg = match theme::active_theme() {
        theme::ThemeMode::Light => Color::Rgb(58, 56, 48),
        theme::ThemeMode::Auto | theme::ThemeMode::Dark | theme::ThemeMode::HighContrast => {
            p.surface_alt
        }
    };
    let mut lines = Vec::new();
    let use_chinese = option_panel_uses_chinese(title, options);
    let prompt = questionnaire_prompt(title);
    let topic = questionnaire_topic(title, use_chinese);
    let labels = decision_labels(kind, use_chinese);

    lines.push(Line::from(vec![
        Span::styled(
            labels.header,
            Style::default()
                .fg(labels.color(&p))
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(labels.prefix, Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            match kind {
                DecisionKind::PlanAction => prompt,
                DecisionKind::Clarification if use_chinese => format!("\"1. [问题]\" {prompt}"),
                DecisionKind::Clarification => format!("\"1. [question]\" {prompt}"),
                DecisionKind::Conflict if use_chinese => format!("[处理] {prompt}"),
                DecisionKind::Conflict => format!("[resolve] {prompt}"),
            },
            Style::default().fg(p.dim).bg(p.canvas),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("          ", Style::default().bg(p.canvas)),
        Span::styled(
            if use_chinese {
                format!("[主题] {topic}")
            } else {
                format!("[topic] {topic}")
            },
            Style::default().fg(p.secondary).bg(p.canvas),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("          ", Style::default().bg(p.canvas)),
        Span::styled(
            labels.options_hint,
            Style::default().fg(p.muted).bg(p.canvas),
        ),
    ]));

    for (i, option) in options.iter().enumerate() {
        let selected = i == selected_index.min(options.len().saturating_sub(1));
        let key = if i < 9 {
            format!("{}", i + 1)
        } else {
            format!("{}", (b'a' + (i as u8 - 9)) as char)
        };
        let row_width = area.width.saturating_sub(4) as usize;
        let desc = truncate(option, row_width.saturating_sub(10));
        let marker = if selected { "▶" } else { " " };
        let row_text = pad_to_width(
            &format!(" {marker} {key}. [{}] {desc}", labels.row_tag),
            row_width,
        );
        let row_style = if selected {
            Style::default()
                .fg(p.inverse_text)
                .bg(selected_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.text).bg(p.canvas)
        };
        lines.push(Line::from(vec![Span::styled(row_text, row_style)]));
    }

    lines.push(Line::from(vec![
        Span::styled(
            "↑↓",
            Style::default()
                .fg(p.accent)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese {
                " 移动   "
            } else {
                " move   "
            },
            Style::default().fg(p.dim).bg(p.canvas),
        ),
        Span::styled(
            "Enter",
            Style::default()
                .fg(p.accent)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(labels.enter_hint, Style::default().fg(p.dim).bg(p.canvas)),
        Span::styled("Esc", Style::default().fg(p.muted).bg(p.canvas)),
        Span::styled(labels.cancel_hint, Style::default().fg(p.dim).bg(p.canvas)),
        Span::styled("1-9", Style::default().fg(p.muted).bg(p.canvas)),
        Span::styled(
            if use_chinese {
                " 快捷键"
            } else {
                " shortcuts"
            },
            Style::default().fg(p.dim).bg(p.canvas),
        ),
    ]));

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::default().fg(p.text).bg(p.canvas))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

struct DecisionLabels {
    header: &'static str,
    prefix: &'static str,
    options_hint: &'static str,
    row_tag: &'static str,
    enter_hint: &'static str,
    cancel_hint: &'static str,
    severity: DecisionSeverity,
}

enum DecisionSeverity {
    Accent,
    Warning,
    Danger,
}

impl DecisionLabels {
    fn color(&self, p: &theme::ThemePalette) -> Color {
        match self.severity {
            DecisionSeverity::Accent => p.accent,
            DecisionSeverity::Warning => p.warning,
            DecisionSeverity::Danger => p.danger,
        }
    }
}

fn decision_labels(kind: DecisionKind, use_chinese: bool) -> DecisionLabels {
    match (kind, use_chinese) {
        (DecisionKind::PlanAction, true) => DecisionLabels {
            header: "计划操作",
            prefix: " 动作: ",
            options_hint: "[执行选项...]",
            row_tag: "执行",
            enter_hint: " 执行所选   ",
            cancel_hint: " 取消计划   ",
            severity: DecisionSeverity::Accent,
        },
        (DecisionKind::PlanAction, false) => DecisionLabels {
            header: "Plan Action",
            prefix: " action: ",
            options_hint: "[actions...]",
            row_tag: "act",
            enter_hint: " run selected   ",
            cancel_hint: " cancel plan   ",
            severity: DecisionSeverity::Accent,
        },
        (DecisionKind::Clarification, true) => DecisionLabels {
            header: "需要补充",
            prefix: " 问题: ",
            options_hint: "[选项...]",
            row_tag: "选",
            enter_hint: " 发送所选   ",
            cancel_hint: " 取消   ",
            severity: DecisionSeverity::Warning,
        },
        (DecisionKind::Clarification, false) => DecisionLabels {
            header: "Need Input",
            prefix: " question: ",
            options_hint: "[options...]",
            row_tag: "Q",
            enter_hint: " send selected   ",
            cancel_hint: " dismiss   ",
            severity: DecisionSeverity::Warning,
        },
        (DecisionKind::Conflict, true) => DecisionLabels {
            header: "需要处理",
            prefix: " 阻断: ",
            options_hint: "[处理选项...]",
            row_tag: "处理",
            enter_hint: " 处理所选   ",
            cancel_hint: " 暂停   ",
            severity: DecisionSeverity::Danger,
        },
        (DecisionKind::Conflict, false) => DecisionLabels {
            header: "Action Needed",
            prefix: " blocked: ",
            options_hint: "[resolutions...]",
            row_tag: "fix",
            enter_hint: " resolve selected   ",
            cancel_hint: " pause   ",
            severity: DecisionSeverity::Danger,
        },
    }
}

fn questionnaire_prompt(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        "Choose how to continue".to_string()
    } else if let Some((_, rest)) = trimmed.split_once(':') {
        truncate(rest.trim(), 54)
    } else {
        truncate(trimmed, 54)
    }
}

fn questionnaire_topic(title: &str, use_chinese: bool) -> String {
    let lower = title.to_ascii_lowercase();
    if use_chinese {
        "计划执行".to_string()
    } else if lower.contains("plan") {
        "plan execution".to_string()
    } else if lower.contains("permission") || lower.contains("approval") {
        "permissions".to_string()
    } else {
        "decision".to_string()
    }
}

fn option_panel_uses_chinese(title: &str, options: &[String]) -> bool {
    contains_cjk(title) || options.iter().any(|option| contains_cjk(option))
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

/// Render slash-command suggestions while the user is typing `/`.
pub fn render_slash_command_panel(
    f: &mut Frame,
    area: Rect,
    commands: &[(String, String)],
    selected_index: usize,
) {
    if area.width < 18 || area.height < 3 || commands.is_empty() {
        return;
    }

    let p = theme::palette();
    let selected_bg = match theme::active_theme() {
        theme::ThemeMode::Light => Color::Rgb(58, 56, 48),
        theme::ThemeMode::Auto | theme::ThemeMode::Dark | theme::ThemeMode::HighContrast => {
            p.surface_alt
        }
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "Commands",
            Style::default()
                .fg(p.text)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default().bg(p.canvas)),
        Span::styled(
            "↑↓ choose   Enter run exact / complete partial   Tab complete",
            Style::default().fg(p.dim).bg(p.canvas),
        ),
    ])];

    let name_width = commands
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, 18);
    let row_width = area.width.saturating_sub(4) as usize;
    let desc_width = row_width.saturating_sub(name_width + 5);

    for (i, (name, description)) in commands.iter().enumerate() {
        let selected = i == selected_index.min(commands.len().saturating_sub(1));
        let marker = if selected { "▸" } else { " " };
        let name_cell = format!("{name:<name_width$}");
        let desc = truncate(description, desc_width);
        let style = if selected {
            Style::default()
                .fg(p.inverse_text)
                .bg(selected_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.text).bg(p.canvas)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), style),
            Span::styled(name_cell, style),
            Span::styled("  ", style),
            Span::styled(desc, style),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::default().fg(p.text).bg(p.canvas))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn divider_line(width: u16) -> Line<'static> {
    let w = width as usize;
    Line::from(vec![Span::styled(
        "─".repeat(w.saturating_sub(1)),
        Style::default().fg(theme::palette().divider),
    )])
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn pad_to_width(value: &str, width: usize) -> String {
    let used = value.chars().count();
    if used >= width {
        value.to_string()
    } else {
        format!("{value}{}", " ".repeat(width - used))
    }
}

/// ━━━●─── style progress bar.
fn progress_bar(steps: &[PlanStepItem], total_steps: usize, width: usize) -> String {
    let total = total_steps.max(steps.len());
    if total == 0 || width < 3 {
        return String::new();
    }

    // Map each step slot to a character
    (0..total)
        .map(|i| match steps.get(i).map(|s| s.status) {
            Some(PlanStepStatus::Done) => '━',
            Some(PlanStepStatus::Running) => '●',
            Some(PlanStepStatus::Failed) => '✗',
            _ => '─',
        })
        .collect()
}

fn step_kind(description: &str) -> &'static str {
    if description.starts_with("agent ") {
        "agent"
    } else if description.starts_with("Read `") {
        "read"
    } else if description.starts_with("Search `") {
        "search"
    } else if description.starts_with("Edit `") {
        "edit"
    } else if description.starts_with("Run `") {
        "run"
    } else if description.starts_with("Verify") {
        "verify"
    } else {
        "task"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn progress_bar_renders_correct_chars() {
        let steps = vec![PlanStepItem::new("read", PlanStepStatus::Done)];
        assert_eq!(progress_bar(&steps, 3, 10), "━──");
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("计划任务界面", 4), "计划任…");
    }

    #[test]
    fn step_kind_detects_plan_executor_labels() {
        assert_eq!(step_kind("Read `src/main.rs` — inspect"), "read");
        assert_eq!(step_kind("Search `router` — from plan"), "search");
        assert_eq!(step_kind("Edit `src/main.rs` — fix"), "edit");
        assert_eq!(step_kind("Run `cargo test` — verify"), "run");
        assert_eq!(step_kind("Verify — run checks"), "verify");
        assert_eq!(step_kind("agent reviewer · 审查关键风险"), "agent");
        assert_eq!(step_kind("Discuss next step"), "task");
    }

    #[test]
    fn status_label_uses_shared_task_words() {
        assert_eq!(status_label(PlanStepStatus::Pending), "queued");
        assert_eq!(status_label(PlanStepStatus::Running), "running");
        assert_eq!(status_label(PlanStepStatus::Done), "done");
        assert_eq!(status_label(PlanStepStatus::Failed), "failed");
    }

    #[test]
    fn duration_formatting_scales_from_seconds_to_hours() {
        assert_eq!(format_duration_compact(900), "<1s");
        assert_eq!(format_duration_compact(38_000), "38s");
        assert_eq!(format_duration_compact(80_000), "1m 20s");
        assert_eq!(format_duration_compact(3_661_000), "1h 1m 1s");
    }

    #[test]
    fn duration_label_distinguishes_running_and_done() {
        let running =
            PlanStepItem::new("Run checks", PlanStepStatus::Running).with_duration_ms(6_000);
        let done = PlanStepItem::new("Run checks", PlanStepStatus::Done).with_duration_ms(55_000);

        assert_eq!(duration_label(&running), " · 6s");
        assert_eq!(duration_label(&done), " · took 55s");
    }

    #[test]
    fn options_panel_is_visibly_a_picker() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
        terminal
            .draw(|f| {
                render_options_panel(
                    f,
                    f.area(),
                    DecisionKind::Clarification,
                    "Tell me more:",
                    &[
                        "Use Python subprocess".to_string(),
                        "Use HTTP mock tests".to_string(),
                        "Check API side effects".to_string(),
                    ],
                    1,
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
        assert!(rendered.contains("Need Input"));
        assert!(rendered.contains("question:"));
        assert!(rendered.contains("[topic] decision"));
        assert!(rendered.contains("▶ 2"));
        assert!(rendered.contains("Enter"));
        assert!(rendered.contains("send selected"));
    }

    #[test]
    fn options_panel_uses_chinese_when_title_is_chinese() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
        terminal
            .draw(|f| {
                render_options_panel(
                    f,
                    f.area(),
                    DecisionKind::PlanAction,
                    "计划执行：测试 CLI",
                    &[
                        "自动执行（包含命令）".to_string(),
                        "需要确认后执行".to_string(),
                        "取消".to_string(),
                    ],
                    0,
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
        assert!(compact.contains("计划操作"));
        assert!(compact.contains("动作"));
        assert!(compact.contains("[主题]计划执行"));
        assert!(compact.contains("[执行]自动执行"));
        assert!(!rendered.contains("Ask User"));
        assert!(!rendered.contains("questionnaire"));
    }

    #[test]
    fn clarification_panel_is_the_only_question_style_decision() {
        theme::set_active_theme(theme::ThemeMode::Light);
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
        terminal
            .draw(|f| {
                render_options_panel(
                    f,
                    f.area(),
                    DecisionKind::Clarification,
                    "需要更多信息：选择测试范围",
                    &["只跑单元测试".to_string(), "跑全部测试".to_string()],
                    0,
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
        assert!(compact.contains("需要补充"));
        assert!(compact.contains("问题"));
        assert!(compact.contains("[选]只跑单元测试"));
    }

    #[test]
    fn questionnaire_helpers_extract_plan_prompt_and_topic() {
        assert_eq!(
            questionnaire_prompt("Plan execution: Build command mode"),
            "Build command mode"
        );
        assert_eq!(
            questionnaire_topic("Plan execution: Build command mode", false),
            "plan execution"
        );
        assert_eq!(questionnaire_topic("计划执行：创建任务", true), "计划执行");
    }
}

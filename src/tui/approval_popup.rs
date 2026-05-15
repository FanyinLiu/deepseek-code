use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};

use crate::policy::{ApprovalDisplay, RiskLevel};
use crate::tui::theme;

/// Render a Droid-style approval sheet anchored above the input area.
pub fn render_approval_popup(f: &mut Frame, area: Rect, approval: &ApprovalDisplay) {
    let p = theme::palette();
    let use_chinese = approval_uses_chinese(approval);

    // Collect owned pairs so temporaries outlive the lines vec.
    let detail_pairs: Vec<(String, String)> = approval
        .details
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let t = line.trim();
            t.split_once(':').map(|(label, value)| {
                (
                    format!(" {}", label.trim().to_ascii_lowercase()),
                    value.trim().to_string(),
                )
            })
        })
        .collect();

    let popup_width = std::cmp::min(80, area.width.saturating_sub(4));
    let requested_height = (6 + detail_pairs.len() as u16).clamp(8, 14);
    let popup_height = std::cmp::min(requested_height, area.height.saturating_sub(4));
    let popup_area = bottom_sheet_rect(area, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let risk_color = match approval.risk_level {
        RiskLevel::SafeRead => p.success,
        RiskLevel::SensitiveRead => p.warning,
        RiskLevel::WriteProject => p.warning,
        RiskLevel::GitMutation => p.accent,
        RiskLevel::CommandExecution => p.danger,
        RiskLevel::NetworkAccess => p.danger,
        RiskLevel::Blocked => p.danger,
    };

    let w = popup_area.width as usize;
    let divider: String = "─".repeat(w.saturating_sub(1));

    let mut lines: Vec<Line> = Vec::new();

    // ── Header: "─── approve tool call · Risk ───"
    let risk_label = risk_label(&approval.risk_level, use_chinese);
    let header_label = if use_chinese {
        "审批工具调用"
    } else {
        "approve tool call"
    };
    let header_text = format!("{header_label} · {risk_label}");
    let fill = w.saturating_sub(header_text.chars().count() + 8);
    lines.push(Line::from(vec![
        Span::styled("─── ", Style::default().fg(p.divider)),
        Span::styled(
            header_label,
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(p.dim)),
        Span::styled(
            risk_label,
            Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", "─".repeat(fill.max(1))),
            Style::default().fg(p.divider),
        ),
    ]));

    // ── Detail rows ──
    lines.push(kv(
        if use_chinese { " 工具" } else { " tool" },
        &approval.title,
        p.secondary,
        p.text,
    ));
    lines.push(kv(
        if use_chinese { " 意图" } else { " intent" },
        &approval.description,
        p.secondary,
        p.text,
    ));
    for (label, value) in &detail_pairs {
        lines.push(kv(label, value, p.secondary, p.text));
    }

    // ── Divider before actions ──
    lines.push(Line::from(vec![Span::styled(
        divider.clone(),
        Style::default().fg(p.divider),
    )]));

    // ── Action bar: [a] [s] [d] with DROID_ACCENT for keys ──
    lines.push(Line::from(vec![
        Span::styled(" [", Style::default().fg(p.dim)),
        Span::styled(
            "a",
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese {
                "] 批准一次  ["
            } else {
                "] approve once  ["
            },
            Style::default().fg(p.dim),
        ),
        Span::styled(
            "s",
            Style::default().fg(p.warning).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese {
                "] 本轮批准  ["
            } else {
                "] approve session  ["
            },
            Style::default().fg(p.dim),
        ),
        Span::styled(
            "d",
            Style::default().fg(p.danger).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if use_chinese { "] 拒绝" } else { "] deny" },
            Style::default().fg(p.dim),
        ),
    ]));

    // ── Bottom divider ──
    lines.push(Line::from(vec![Span::styled(
        divider,
        Style::default().fg(p.divider),
    )]));

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(p.canvas))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, popup_area);
}

fn approval_uses_chinese(approval: &ApprovalDisplay) -> bool {
    contains_cjk(&approval.title)
        || contains_cjk(&approval.description)
        || contains_cjk(&approval.details)
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
        )
    })
}

fn risk_label(risk: &RiskLevel, use_chinese: bool) -> String {
    if !use_chinese {
        return risk.to_string();
    }
    match risk {
        RiskLevel::SafeRead => "安全读取",
        RiskLevel::SensitiveRead => "敏感读取",
        RiskLevel::WriteProject => "写入项目",
        RiskLevel::GitMutation => "Git 修改",
        RiskLevel::CommandExecution => "执行命令",
        RiskLevel::NetworkAccess => "网络访问",
        RiskLevel::Blocked => "已阻止",
    }
    .to_string()
}

fn kv<'a>(
    label: &'a str,
    value: &'a str,
    label_color: ratatui::style::Color,
    value_color: ratatui::style::Color,
) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), Style::default().fg(label_color)),
        Span::styled(value, Style::default().fg(value_color)),
    ])
}

fn bottom_sheet_rect(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let bottom_margin = if area.height > height.saturating_add(4) {
        4
    } else {
        0
    };
    let y = area.y
        + area
            .height
            .saturating_sub(height.saturating_add(bottom_margin));
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn approval_dialog_includes_risk_path_command() {
        let approval = ApprovalDisplay {
            title: "Run Command".to_string(),
            description: "cargo test".to_string(),
            risk_level: RiskLevel::CommandExecution,
            details: "Command: cargo test\nCWD: project root\nPath: src/main.rs".to_string(),
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 28)).expect("terminal");
        terminal
            .draw(|f| render_approval_popup(f, f.area(), &approval))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(text.contains("approve tool call"));
        assert!(text.contains("CommandExecution"));
        assert!(text.contains("cargo test"));
        assert!(text.contains("src/main.rs"));
    }

    #[test]
    fn chinese_approval_dialog_uses_chinese_actions() {
        let approval = ApprovalDisplay {
            title: "运行命令".to_string(),
            description: "执行 cargo test".to_string(),
            risk_level: RiskLevel::CommandExecution,
            details: "来源: 主 agent\n命令: cargo test".to_string(),
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 28)).expect("terminal");
        terminal
            .draw(|f| render_approval_popup(f, f.area(), &approval))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        let compact_text = text.split_whitespace().collect::<String>();

        assert!(compact_text.contains("审批工具调用"));
        assert!(compact_text.contains("执行命令"));
        assert!(compact_text.contains("批准一次"));
        assert!(compact_text.contains("本轮批准"));
        assert!(compact_text.contains("拒绝"));
        assert!(!text.contains("approve once"));
        assert!(!text.contains("approve session"));
    }

    #[test]
    fn approval_dialog_is_bottom_anchored() {
        let rect = bottom_sheet_rect(Rect::new(0, 0, 100, 30), 80, 9);
        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 17);
    }
}

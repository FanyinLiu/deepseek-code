use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};

use crate::policy::{ApprovalDisplay, RiskLevel};
use crate::tui::theme;

/// Height an inline approval needs in the layout for a given approval state.
#[must_use]
pub fn inline_height(approval: &ApprovalDisplay) -> u16 {
    let detail_rows = detail_pairs(approval).len() as u16;
    // header + tool + intent + details + action bar
    3 + detail_rows + 1
}

/// Render the approval inline, flowing with the layout rather than overlaying.
pub fn render_approval_inline(f: &mut Frame, area: Rect, approval: &ApprovalDisplay) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let p = theme::palette();
    let use_chinese = approval_uses_chinese(approval);
    let details = detail_pairs(approval);

    let risk_color = match approval.risk_level {
        RiskLevel::SafeRead => p.success,
        RiskLevel::SensitiveRead | RiskLevel::WriteProject => p.warning,
        RiskLevel::GitMutation => p.accent,
        RiskLevel::CommandExecution | RiskLevel::NetworkAccess | RiskLevel::Blocked => p.danger,
    };

    let risk_label = risk_label(&approval.risk_level, use_chinese);
    let header_label = if use_chinese {
        "审批工具调用"
    } else {
        "approve tool call"
    };

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            header_label,
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(p.dim)),
        Span::styled(
            risk_label,
            Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
        ),
    ]));

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
    for (label, value) in &details {
        lines.push(kv(label, value, p.secondary, p.text));
    }

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

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}

/// Legacy overlay used by the fullscreen renderer. Classic mode renders the
/// approval inline via [`render_approval_inline`] instead.
pub fn render_approval_popup(f: &mut Frame, area: Rect, approval: &ApprovalDisplay) {
    let height = inline_height(approval).clamp(6, area.height.saturating_sub(4).max(6));
    let width = std::cmp::min(80, area.width.saturating_sub(4));
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
    let popup_area = Rect::new(x, y, width, height);
    f.render_widget(Clear, popup_area);
    render_approval_inline(f, popup_area, approval);
}

fn detail_pairs(approval: &ApprovalDisplay) -> Vec<(String, String)> {
    approval
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
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_text(approval: &ApprovalDisplay, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| render_approval_inline(f, f.area(), approval))
            .expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
    }

    #[test]
    fn inline_approval_shows_risk_path_command() {
        let approval = ApprovalDisplay {
            title: "Run Command".to_string(),
            description: "cargo test".to_string(),
            risk_level: RiskLevel::CommandExecution,
            details: "Command: cargo test\nCWD: project root\nPath: src/main.rs".to_string(),
        };
        let text = render_text(&approval, 100, inline_height(&approval));

        assert!(text.contains("approve tool call"));
        assert!(text.contains("CommandExecution"));
        assert!(text.contains("cargo test"));
        assert!(text.contains("src/main.rs"));
    }

    #[test]
    fn inline_approval_uses_chinese_actions() {
        let approval = ApprovalDisplay {
            title: "运行命令".to_string(),
            description: "执行 cargo test".to_string(),
            risk_level: RiskLevel::CommandExecution,
            details: "来源: 主 agent\n命令: cargo test".to_string(),
        };
        let text = render_text(&approval, 100, inline_height(&approval));
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
    fn inline_height_accounts_for_detail_rows() {
        let approval = ApprovalDisplay {
            title: "t".into(),
            description: "d".into(),
            risk_level: RiskLevel::SafeRead,
            details: "Path: a\nCWD: b".into(),
        };
        // header + tool + intent + 2 details + action bar = 6
        assert_eq!(inline_height(&approval), 6);
    }
}

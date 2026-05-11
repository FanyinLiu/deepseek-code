use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};

use crate::policy::{ApprovalDisplay, RiskLevel};
use crate::tui::theme;

/// Render a Droid-style approval popup: top/bottom dividers, flat rows, accent action keys.
pub fn render_approval_popup(f: &mut Frame, area: Rect, approval: &ApprovalDisplay) {
    let p = theme::palette();
    let popup_width = std::cmp::min(80, area.width.saturating_sub(4));
    let popup_height = std::cmp::min(14, area.height.saturating_sub(4));
    let popup_area = centered_rect(area, popup_width, popup_height);

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
    let risk_label = approval.risk_level.to_string();
    let header_text = format!("approve tool call · {risk_label}");
    let fill = w.saturating_sub(header_text.chars().count() + 8);
    lines.push(Line::from(vec![
        Span::styled("─── ", Style::default().fg(p.divider)),
        Span::styled(
            "approve tool call",
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

    lines.push(kv(" tool", &approval.title, p.secondary, p.text));
    lines.push(kv(" intent", &approval.description, p.secondary, p.text));
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
        Span::styled("] approve once  [", Style::default().fg(p.dim)),
        Span::styled(
            "s",
            Style::default().fg(p.warning).add_modifier(Modifier::BOLD),
        ),
        Span::styled("] approve session  [", Style::default().fg(p.dim)),
        Span::styled(
            "d",
            Style::default().fg(p.danger).add_modifier(Modifier::BOLD),
        ),
        Span::styled("] deny", Style::default().fg(p.dim)),
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

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
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
}

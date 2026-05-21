use std::io;

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal, TerminalOptions, Viewport,
};

use crate::policy::ApprovalDisplay;
use crate::tui::{approval_popup, select_popup};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalChoice {
    ApproveOnce,
    ApproveSession,
    Deny,
}

pub async fn prompt_approval_inline(approval: &ApprovalDisplay) -> std::io::Result<ApprovalChoice> {
    let _raw_mode = RawModeGuard::new()?;
    let backend = CrosstermBackend::new(std::io::stderr());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(approval_popup_height(approval)),
        },
    )?;

    let result = run_approval_loop(&mut terminal, approval).await;
    let clear_result = terminal.clear();
    clear_result?;
    result
}

pub async fn prompt_select_inline(
    title: &str,
    options: &[String],
) -> std::io::Result<Option<usize>> {
    if options.is_empty() {
        return Ok(None);
    }

    let _raw_mode = RawModeGuard::new()?;
    let backend = CrosstermBackend::new(std::io::stderr());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(select_popup_height(options)),
        },
    )?;

    let result = run_select_loop(&mut terminal, title, options).await;
    let clear_result = terminal.clear();
    clear_result?;
    result
}

async fn run_approval_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stderr>>,
    approval: &ApprovalDisplay,
) -> io::Result<ApprovalChoice> {
    let mut selected_index = 0usize;
    let mut events = EventStream::new();
    loop {
        draw_approval(terminal, approval, selected_index)?;
        match events.next().await {
            Some(Ok(Event::Key(key))) if is_actionable_key(key) => {
                if let Some(choice) = handle_approval_key(key, &mut selected_index) {
                    return Ok(choice);
                }
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(error),
            None => return Ok(ApprovalChoice::Deny),
        }
    }
}

async fn run_select_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stderr>>,
    title: &str,
    options: &[String],
) -> io::Result<Option<usize>> {
    let mut selected_index = 0usize;
    let mut events = EventStream::new();
    loop {
        draw_select(terminal, title, options, selected_index)?;
        match events.next().await {
            Some(Ok(Event::Key(key))) if is_actionable_key(key) => {
                if let Some(choice) = handle_select_key(key, options.len(), &mut selected_index) {
                    return Ok(choice);
                }
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(error),
            None => return Ok(None),
        }
    }
}

fn draw_approval<B: Backend>(
    terminal: &mut Terminal<B>,
    approval: &ApprovalDisplay,
    selected_index: usize,
) -> Result<(), B::Error> {
    terminal
        .draw(|f| approval_popup::render_approval_popup(f, f.area(), approval, selected_index))
        .map(|_| ())
}

fn draw_select<B: Backend>(
    terminal: &mut Terminal<B>,
    title: &str,
    options: &[String],
    selected_index: usize,
) -> Result<(), B::Error> {
    terminal
        .draw(|f| select_popup::render_select_popup(f, f.area(), title, options, selected_index))
        .map(|_| ())
}

fn handle_approval_key(key: KeyEvent, selected_index: &mut usize) -> Option<ApprovalChoice> {
    match key.code {
        KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
            let count = approval_popup::APPROVAL_ACTION_COUNT;
            *selected_index = if *selected_index == 0 {
                count.saturating_sub(1)
            } else {
                (*selected_index).saturating_sub(1)
            };
            None
        }
        KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
            let count = approval_popup::APPROVAL_ACTION_COUNT.max(1);
            *selected_index = (*selected_index + 1) % count;
            None
        }
        KeyCode::Enter => Some(approval_choice_from_index(*selected_index)),
        KeyCode::Esc => Some(ApprovalChoice::Deny),
        KeyCode::Char(c) if c.eq_ignore_ascii_case(&'a') => Some(ApprovalChoice::ApproveOnce),
        KeyCode::Char(c) if c.eq_ignore_ascii_case(&'s') => Some(ApprovalChoice::ApproveSession),
        KeyCode::Char(c) if c.eq_ignore_ascii_case(&'d') => Some(ApprovalChoice::Deny),
        _ => None,
    }
}

fn handle_select_key(
    key: KeyEvent,
    option_count: usize,
    selected_index: &mut usize,
) -> Option<Option<usize>> {
    if option_count == 0 {
        return Some(None);
    }

    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            *selected_index = if *selected_index == 0 {
                option_count.saturating_sub(1)
            } else {
                (*selected_index).saturating_sub(1)
            };
            None
        }
        KeyCode::Down | KeyCode::Tab => {
            *selected_index = (*selected_index + 1) % option_count;
            None
        }
        KeyCode::Enter => Some(Some((*selected_index).min(option_count.saturating_sub(1)))),
        KeyCode::Esc => Some(None),
        KeyCode::Char(c) => option_shortcut_index(c, option_count).map(Some),
        _ => None,
    }
}

fn approval_choice_from_index(index: usize) -> ApprovalChoice {
    match index {
        0 => ApprovalChoice::ApproveOnce,
        1 => ApprovalChoice::ApproveSession,
        _ => ApprovalChoice::Deny,
    }
}

fn option_shortcut_index(c: char, option_count: usize) -> Option<usize> {
    c.to_digit(10)
        .and_then(|digit| usize::try_from(digit).ok())
        .and_then(|digit| digit.checked_sub(1))
        .filter(|index| *index < option_count)
}

fn is_actionable_key(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn approval_popup_height(approval: &ApprovalDisplay) -> u16 {
    let details = approval
        .details
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u16;
    (8 + details).clamp(12, 18)
}

fn select_popup_height(options: &[String]) -> u16 {
    (options.len() as u16 + 5).clamp(8, 18)
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::RiskLevel;
    use crossterm::event::KeyModifiers;
    use ratatui::{backend::TestBackend, Terminal};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn approval_headless_keys_return_session_choice() {
        let approval = ApprovalDisplay {
            title: "Run Command".to_string(),
            description: "cargo test".to_string(),
            risk_level: RiskLevel::CommandExecution,
            details: "Command: cargo test".to_string(),
        };
        let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("terminal");

        let choice = drive_approval_keys(
            &mut terminal,
            &approval,
            &[key(KeyCode::Right), key(KeyCode::Enter)],
        )
        .expect("drive keys");

        assert_eq!(choice, ApprovalChoice::ApproveSession);
    }

    #[test]
    fn select_headless_keys_return_selected_index() {
        let options = vec!["one".to_string(), "two".to_string(), "three".to_string()];
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).expect("terminal");

        let choice = drive_select_keys(
            &mut terminal,
            "Pick",
            &options,
            &[key(KeyCode::Down), key(KeyCode::Enter)],
        )
        .expect("drive keys");

        assert_eq!(choice, Some(1));
    }

    fn drive_approval_keys<B: Backend>(
        terminal: &mut Terminal<B>,
        approval: &ApprovalDisplay,
        keys: &[KeyEvent],
    ) -> Result<ApprovalChoice, B::Error> {
        let mut selected_index = 0usize;
        for key in keys {
            draw_approval(terminal, approval, selected_index)?;
            if let Some(choice) = handle_approval_key(*key, &mut selected_index) {
                return Ok(choice);
            }
        }
        draw_approval(terminal, approval, selected_index)?;
        Ok(ApprovalChoice::Deny)
    }

    fn drive_select_keys<B: Backend>(
        terminal: &mut Terminal<B>,
        title: &str,
        options: &[String],
        keys: &[KeyEvent],
    ) -> Result<Option<usize>, B::Error> {
        let mut selected_index = 0usize;
        for key in keys {
            draw_select(terminal, title, options, selected_index)?;
            if let Some(choice) = handle_select_key(*key, options.len(), &mut selected_index) {
                return Ok(choice);
            }
        }
        draw_select(terminal, title, options, selected_index)?;
        Ok(None)
    }
}

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};

/// Main application layout.
/// Returns: (status_area, main_content_area, model_hint_area, divider_area, input_area, footer_area)
#[must_use]
pub fn app_layout(area: Rect, input_height: u16) -> (Rect, Rect, Rect, Rect, Rect, Rect) {
    let h = input_height.clamp(1, 5);

    // Add outer margin for "floating" feel
    let outer = area.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Length(1), // spacer (top)
            Constraint::Min(5),    // main content
            Constraint::Length(1), // active model hint
            Constraint::Length(1), // divider line above input
            Constraint::Length(h), // input area
            Constraint::Length(2), // compact footer
        ])
        .split(outer);

    (
        chunks[0], chunks[2], chunks[3], chunks[4], chunks[5], chunks[6],
    )
}

/// Three-column layout for search/browse mode.
#[allow(dead_code)]
#[must_use]
pub fn search_layout(area: Rect) -> (Rect, Rect, Rect) {
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(45),
            Constraint::Percentage(30),
        ])
        .split(inner);
    (chunks[0], chunks[1], chunks[2])
}

/// Split main area into sidebar + content.
#[must_use]
pub fn sidebar_layout(area: Rect, sidebar_width: u16) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(10)])
        .split(area);
    (chunks[0], chunks[1])
}

/// Add inner padding to a rect for card content.
#[must_use]
pub fn card_inner(area: Rect) -> Rect {
    area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    })
}

/// Horizontal spacer height.
pub const SPACER: Constraint = Constraint::Length(1);

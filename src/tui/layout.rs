use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};

/// Width discipline keeps content within a comfortable reading width while
/// preserving terminal breathing room on large screens.
pub const CONTENT_MAX_WIDTH: u16 = 100;

/// Main application layout.
/// Returns: (status_area, main_content_area, model_hint_area, divider_area, input_area, footer_area)
#[must_use]
pub fn app_layout(area: Rect, input_height: u16) -> (Rect, Rect, Rect, Rect, Rect, Rect) {
    let h = input_height.clamp(1, 4);
    let footer_height = 1;
    let show_model_hint = area.width >= 96 && area.height >= 20;
    let reserved = 1 + u16::from(show_model_hint) + 1 + h + footer_height;

    let content_min = (area.height.saturating_sub(reserved)).max(2);

    // Cap horizontal width and center the column; on narrow terminals this is
    // a no-op (uses the full area).
    let outer = centered_column(area, CONTENT_MAX_WIDTH);

    let constraints = if show_model_hint {
        vec![
            Constraint::Length(1),
            Constraint::Min(content_min),
            Constraint::Length(1),
            Constraint::Length(h),
            Constraint::Length(footer_height),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Min(content_min),
            Constraint::Length(h),
            Constraint::Length(footer_height),
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(outer);

    let mut idx = 0;
    let status_area = chunks[idx];
    idx += 1;
    let main_content_area = chunks[idx];
    idx += 1;
    let model_hint_area = if show_model_hint {
        let rect = chunks[idx];
        idx += 1;
        rect
    } else {
        Rect::new(0, 0, 0, 0)
    };

    let divider_area = Rect::new(0, 0, 0, 0);
    let input_area = chunks[idx];
    idx += 1;
    let footer_area = chunks[idx];

    (
        status_area,
        main_content_area,
        model_hint_area,
        divider_area,
        input_area,
        footer_area,
    )
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

/// Center a fixed-width column inside `area`. If `area` is narrower than
/// `max_width`, returns `area` unchanged.
#[must_use]
pub fn centered_column(area: Rect, max_width: u16) -> Rect {
    if area.width <= max_width {
        return area;
    }
    let left_pad = (area.width - max_width) / 2;
    Rect::new(area.x + left_pad, area.y, max_width, area.height)
}

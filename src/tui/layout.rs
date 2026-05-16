use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};

/// Main application layout.
/// Returns: (status_area, main_content_area, model_hint_area, divider_area, input_area, footer_area)
#[must_use]
pub fn app_layout(area: Rect, input_height: u16) -> (Rect, Rect, Rect, Rect, Rect, Rect) {
    let h = input_height.clamp(1, 5);
    let mut show_model_hint = true;
    let mut show_status_spacer = true;
    let mut footer_height = 2;

    if area.height < 20 {
        show_status_spacer = false;
    }
    if area.height < 16 {
        show_model_hint = false;
    }
    if area.height < 14 {
        footer_height = 1;
    }

    let reserved =
        1 + u16::from(show_status_spacer) + u16::from(show_model_hint) + 1 + h + footer_height;

    let content_min = (area.height.saturating_sub(reserved)).max(2);

    // Add outer margin for "floating" feel
    let outer = area.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });

    let mut constraints = Vec::with_capacity(6);
    constraints.push(Constraint::Length(1));
    if show_status_spacer {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(content_min));
    if show_model_hint {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(h));
    constraints.push(Constraint::Length(footer_height));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(outer);

    let mut idx = 0;
    let status_area = chunks[idx];
    idx += 1;
    if show_status_spacer {
        idx += 1;
    }
    let main_content_area = chunks[idx];
    idx += 1;
    let model_hint_area = if show_model_hint {
        chunks[idx].to_owned()
    } else {
        Rect::new(0, 0, 0, 0)
    };
    if show_model_hint {
        idx += 1;
    }
    let divider_area = chunks[idx];
    idx += 1;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActiveScreen {
    Decision,
    Approval,
    Settings,
    ApiKeyEntry,
    HistorySearch,
    DiffFocus,
    FileTree,
    Welcome,
    Workbench,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ScreenFlags {
    pub decision_open: bool,
    pub approval_open: bool,
    pub settings_open: bool,
    pub api_key_entry_open: bool,
    pub history_search_open: bool,
    pub diff_focused: bool,
    pub file_tree_focused: bool,
    pub showing_welcome: bool,
}

#[must_use]
pub fn active_screen(flags: ScreenFlags) -> ActiveScreen {
    if flags.decision_open {
        ActiveScreen::Decision
    } else if flags.approval_open {
        ActiveScreen::Approval
    } else if flags.settings_open {
        ActiveScreen::Settings
    } else if flags.api_key_entry_open {
        ActiveScreen::ApiKeyEntry
    } else if flags.history_search_open {
        ActiveScreen::HistorySearch
    } else if flags.diff_focused {
        ActiveScreen::DiffFocus
    } else if flags.file_tree_focused {
        ActiveScreen::FileTree
    } else if flags.showing_welcome {
        ActiveScreen::Welcome
    } else {
        ActiveScreen::Workbench
    }
}

impl ActiveScreen {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Approval => "approval",
            Self::Settings => "settings",
            Self::ApiKeyEntry => "api-key",
            Self::HistorySearch => "history-search",
            Self::DiffFocus => "diff-focus",
            Self::FileTree => "file-tree",
            Self::Welcome => "welcome",
            Self::Workbench => "workbench",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_precedence_keeps_modal_layers_above_workbench_focus() {
        let flags = ScreenFlags {
            decision_open: true,
            approval_open: true,
            settings_open: true,
            diff_focused: true,
            file_tree_focused: true,
            showing_welcome: true,
            ..ScreenFlags::default()
        };

        assert_eq!(active_screen(flags), ActiveScreen::Decision);
    }

    #[test]
    fn active_screen_prefers_transient_focus_before_welcome() {
        assert_eq!(
            active_screen(ScreenFlags {
                diff_focused: true,
                showing_welcome: true,
                ..ScreenFlags::default()
            }),
            ActiveScreen::DiffFocus
        );
        assert_eq!(
            active_screen(ScreenFlags {
                file_tree_focused: true,
                showing_welcome: true,
                ..ScreenFlags::default()
            }),
            ActiveScreen::FileTree
        );
        assert_eq!(
            active_screen(ScreenFlags {
                showing_welcome: true,
                ..ScreenFlags::default()
            }),
            ActiveScreen::Welcome
        );
    }
}

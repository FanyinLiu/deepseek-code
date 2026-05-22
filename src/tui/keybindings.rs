use std::{collections::HashMap, fs, path::Path};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    Exit,
    Interrupt,
    Submit,
    HistoryUp,
    HistoryDown,
    Complete,
    OpenSettings,
    OpenHistorySearch,
}

const ACTION_ORDER: [KeyAction; 8] = [
    KeyAction::Exit,
    KeyAction::Interrupt,
    KeyAction::OpenSettings,
    KeyAction::OpenHistorySearch,
    KeyAction::Submit,
    KeyAction::Complete,
    KeyAction::HistoryUp,
    KeyAction::HistoryDown,
];

impl KeyAction {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match normalize_action_name(name).as_str() {
            "exit" => Some(Self::Exit),
            "interrupt" => Some(Self::Interrupt),
            "submit" => Some(Self::Submit),
            "historyup" | "historyprevious" | "previoushistory" => Some(Self::HistoryUp),
            "historydown" | "historynext" | "nexthistory" => Some(Self::HistoryDown),
            "complete" | "completion" => Some(Self::Complete),
            "opensettings" | "settings" => Some(Self::OpenSettings),
            "openhistorysearch" | "historysearch" | "searchhistory" => {
                Some(Self::OpenHistorySearch)
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn config_name(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            Self::Interrupt => "interrupt",
            Self::Submit => "submit",
            Self::HistoryUp => "history_up",
            Self::HistoryDown => "history_down",
            Self::Complete => "complete",
            Self::OpenSettings => "open_settings",
            Self::OpenHistorySearch => "open_history_search",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyStroke {
    #[must_use]
    pub fn from_event(event: KeyEvent) -> Self {
        Self {
            code: normalize_key_code(event.code),
            modifiers: normalize_modifiers(event.modifiers),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySequence {
    strokes: Vec<KeyStroke>,
}

impl KeySequence {
    #[must_use]
    pub fn first_stroke(&self) -> Option<KeyStroke> {
        self.strokes.first().copied()
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<KeyAction, KeySequence>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        insert_default(&mut bindings, KeyAction::Submit, "enter");
        insert_default(&mut bindings, KeyAction::Exit, "ctrl+d");
        insert_default(&mut bindings, KeyAction::Interrupt, "ctrl+c");
        insert_default(&mut bindings, KeyAction::HistoryUp, "up");
        insert_default(&mut bindings, KeyAction::HistoryDown, "down");
        insert_default(&mut bindings, KeyAction::Complete, "tab");
        insert_default(&mut bindings, KeyAction::OpenSettings, "ctrl+,");
        insert_default(&mut bindings, KeyAction::OpenHistorySearch, "ctrl+r");
        Self { bindings }
    }
}

impl Keymap {
    #[must_use]
    pub fn load_project(project_root: &Path) -> Self {
        let mut keymap = Self::default();
        let path = project_root.join(".octocode").join("keybindings.toml");
        if let Ok(content) = fs::read_to_string(path) {
            keymap.apply_toml_overrides(&content);
        }
        keymap
    }

    pub fn apply_toml_overrides(&mut self, content: &str) {
        let Ok(value) = content.parse::<toml::Value>() else {
            return;
        };
        let Some(table) = value.get("keybindings").and_then(toml::Value::as_table) else {
            return;
        };
        for (name, value) in table {
            let Some(action) = KeyAction::from_name(name) else {
                continue;
            };
            let Some(spec) = value.as_str() else {
                continue;
            };
            if let Some(sequence) = parse_key_sequence(spec) {
                self.bindings.insert(action, sequence);
            }
        }
    }

    #[must_use]
    pub fn action_for(&self, event: KeyEvent) -> Option<KeyAction> {
        let stroke = KeyStroke::from_event(event);
        ACTION_ORDER.into_iter().find(|action| {
            self.bindings
                .get(action)
                .is_some_and(|sequence| sequence.first_stroke() == Some(stroke))
        })
    }

    #[must_use]
    pub fn binding_label(&self, action: KeyAction) -> Option<String> {
        self.bindings.get(&action).map(render_key_sequence)
    }
}

fn insert_default(bindings: &mut HashMap<KeyAction, KeySequence>, action: KeyAction, spec: &str) {
    if let Some(sequence) = parse_key_sequence(spec) {
        bindings.insert(action, sequence);
    }
}

fn normalize_action_name(name: &str) -> String {
    name.chars()
        .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn parse_key_sequence(value: &str) -> Option<KeySequence> {
    let strokes: Vec<KeyStroke> = value
        .split_whitespace()
        .filter_map(parse_key_stroke)
        .collect();
    (!strokes.is_empty()).then_some(KeySequence { strokes })
}

fn parse_key_stroke(value: &str) -> Option<KeyStroke> {
    let mut modifiers = KeyModifiers::empty();
    let mut code_part: Option<&str> = None;
    for part in value.split('+') {
        let normalized = part.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "" => {}
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "option" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            _ => code_part = Some(part.trim()),
        }
    }
    let code = parse_key_code(code_part?)?;
    Some(KeyStroke {
        code,
        modifiers: normalize_modifiers(modifiers),
    })
}

fn parse_key_code(value: &str) -> Option<KeyCode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "enter" | "return" => Some(KeyCode::Enter),
        "esc" | "escape" => Some(KeyCode::Esc),
        "tab" => Some(KeyCode::Tab),
        "backtab" | "shift-tab" | "shift+tab" => Some(KeyCode::BackTab),
        "space" => Some(KeyCode::Char(' ')),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pageup" | "pgup" => Some(KeyCode::PageUp),
        "pagedown" | "pgdn" => Some(KeyCode::PageDown),
        "backspace" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        _ => {
            let mut chars = value.chars();
            let ch = chars.next()?;
            chars.next().is_none().then_some(KeyCode::Char(ch))
        }
    }
}

fn normalize_key_code(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(ch) => KeyCode::Char(ch.to_ascii_lowercase()),
        other => other,
    }
}

fn normalize_modifiers(modifiers: KeyModifiers) -> KeyModifiers {
    modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
}

fn render_key_sequence(sequence: &KeySequence) -> String {
    sequence
        .strokes
        .iter()
        .map(render_key_stroke)
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_key_stroke(stroke: &KeyStroke) -> String {
    let mut parts = Vec::new();
    if stroke.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if stroke.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if stroke.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    parts.push(match stroke.code {
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "backtab".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(ch) => ch.to_string(),
        _ => "?".to_string(),
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
    }

    #[test]
    fn default_keymap_maps_exit_interrupt_and_settings() {
        let keymap = Keymap::default();

        assert_eq!(
            keymap.action_for(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(KeyAction::Exit)
        );
        assert_eq!(
            keymap.action_for(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(KeyAction::Interrupt)
        );
        assert_eq!(
            keymap.action_for(key(KeyCode::Char(','), KeyModifiers::CONTROL)),
            Some(KeyAction::OpenSettings)
        );
    }

    #[test]
    fn toml_overrides_replace_default_bindings() {
        let mut keymap = Keymap::default();
        keymap.apply_toml_overrides(
            r#"
[keybindings]
exit = "ctrl+q"
open_settings = "ctrl+o"
"#,
        );

        assert_eq!(
            keymap.action_for(key(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            Some(KeyAction::Exit)
        );
        assert_eq!(
            keymap.action_for(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            keymap.binding_label(KeyAction::OpenSettings),
            Some("ctrl+o".to_string())
        );
    }

    #[test]
    fn chord_like_specs_keep_first_stroke_for_current_router() {
        let mut keymap = Keymap::default();
        keymap.apply_toml_overrides(
            r#"
[keybindings]
exit = "ctrl+d ctrl+d"
"#,
        );

        assert_eq!(
            keymap.action_for(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(KeyAction::Exit)
        );
        assert_eq!(
            keymap.binding_label(KeyAction::Exit),
            Some("ctrl+d ctrl+d".to_string())
        );
    }
}

use crate::deepseek::{DeepSeekModel, ThinkingMode};

pub struct SettingsSnapshot<'a> {
    pub model: &'a DeepSeekModel,
    pub thinking: &'a ThinkingMode,
    pub theme_label: &'a str,
}

/// Render the read-only settings overview as plain text for the transcript.
#[must_use]
pub fn format_inline(snapshot: SettingsSnapshot<'_>) -> String {
    let mut out = String::new();
    out.push_str("Settings (read-only)\n\n");

    let model = snapshot.model.to_string();
    let thinking = snapshot.thinking.to_string();

    out.push_str("Session defaults\n");
    write_row(&mut out, "Default model", &model);
    write_row(&mut out, "Thinking mode", &thinking);
    write_row(&mut out, "Interaction mode", "auto");
    write_row(&mut out, "Autonomy level", "high");
    write_row(&mut out, "Compaction limit", "1M");

    out.push('\n');
    out.push_str("Task defaults\n");
    write_row(&mut out, "Orchestrator model", &format!("{model} / auto"));
    write_row(&mut out, "Worker model", &format!("{model} / auto"));
    write_row(&mut out, "Verifier model", &format!("{model} / auto"));
    write_row(&mut out, "Skip review", "off");
    write_row(&mut out, "Skip user tests", "off");

    out.push('\n');
    out.push_str("Preferences\n");
    write_row(&mut out, "Diff display mode", "GitHub");
    write_row(&mut out, "Theme", snapshot.theme_label);
    write_row(&mut out, "Tool result display", "Compact");
    write_row(&mut out, "Statusline mode", "Powerline");
    write_row(&mut out, "Statusline icon mode", "Text");
    write_row(&mut out, "Statusline shape", "Capsule");
    write_row(&mut out, "Cursor style", "Inline block");
    write_row(&mut out, "Prompt precache", "on");
    write_row(&mut out, "Startup logo animation", "off");
    write_row(&mut out, "Hooks", "enabled");

    out.push('\n');
    out.push_str("Sound\n");
    write_row(&mut out, "Completion sound", "FX-OK01");
    write_row(&mut out, "Waiting input sound", "FX-ACK01");
    write_row(&mut out, "Play sound", "always");

    out.push_str("\nEditing settings is not yet supported.");
    out
}

fn write_row(out: &mut String, label: &str, value: &str) {
    out.push_str("  ");
    out.push_str(&format!("{label:<22}"));
    out.push_str(value);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_each_section_and_values() {
        let text = format_inline(SettingsSnapshot {
            model: &DeepSeekModel::Flash,
            thinking: &ThinkingMode::Auto,
            theme_label: "light",
        });
        assert!(text.contains("Settings (read-only)"));
        assert!(text.contains("Session defaults"));
        assert!(text.contains("Task defaults"));
        assert!(text.contains("Preferences"));
        assert!(text.contains("Sound"));
        assert!(text.contains("Theme"));
        assert!(text.contains("light"));
        assert!(text.contains("Editing settings is not yet supported."));
    }
}

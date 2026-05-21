#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionLevel {
    Subtle,
    Off,
}

impl MotionLevel {
    #[must_use]
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "false" | "disabled" => Self::Off,
            "subtle" | "on" | "true" | "smooth" | "" => Self::Subtle,
            _ => Self::Subtle,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Subtle => "subtle",
            Self::Off => "off",
        }
    }

    #[must_use]
    pub fn is_enabled(self) -> bool {
        self == Self::Subtle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionFrame {
    pub level: MotionLevel,
    pub elapsed_ms: u64,
}

impl MotionFrame {
    pub const TICK_MS: u64 = 50;

    #[must_use]
    pub fn new(level: MotionLevel, elapsed_ms: u64) -> Self {
        Self { level, elapsed_ms }
    }

    #[must_use]
    pub fn disabled() -> Self {
        Self::new(MotionLevel::Off, 0)
    }

    #[must_use]
    pub fn running_icon(self) -> &'static str {
        if !self.level.is_enabled() {
            return "◉";
        }
        const FRAMES: [&str; 6] = [" ", "◐", "◓", "◑", "◒", "◕"];
        const FULL_PHASES: usize = FRAMES.len() * 2;
        let tick = self.frame_index(FULL_PHASES);
        if tick < FRAMES.len() {
            FRAMES[tick]
        } else {
            FRAMES[FULL_PHASES - tick - 1]
        }
    }

    #[must_use]
    pub fn cursor(self) -> &'static str {
        if !self.level.is_enabled() || self.blink_on(500) {
            "▌"
        } else {
            " "
        }
    }

    #[must_use]
    pub fn stream_cursor(self) -> &'static str {
        if !self.level.is_enabled() || self.blink_on(400) {
            "▌"
        } else {
            " "
        }
    }

    #[must_use]
    pub fn dots(self) -> &'static str {
        if !self.level.is_enabled() {
            return "...";
        }
        const FRAMES: [&str; 4] = [".  ", ".. ", "...", " .."];
        FRAMES[self.frame_index(FRAMES.len())]
    }

    #[must_use]
    pub fn accent_once(self) -> bool {
        self.level.is_enabled() && self.elapsed_ms < 700
    }

    #[must_use]
    pub fn is_stalled(self, threshold_ms: u64) -> bool {
        self.elapsed_ms >= threshold_ms
    }

    fn frame_index(self, len: usize) -> usize {
        ((self.elapsed_ms / Self::TICK_MS) as usize) % len.max(1)
    }

    fn blink_on(self, period_ms: u64) -> bool {
        (self.elapsed_ms / period_ms.max(1)).is_multiple_of(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_level_parses_config_aliases() {
        assert_eq!(MotionLevel::from_config("subtle"), MotionLevel::Subtle);
        assert_eq!(MotionLevel::from_config("smooth"), MotionLevel::Subtle);
        assert_eq!(MotionLevel::from_config("off"), MotionLevel::Off);
        assert_eq!(MotionLevel::from_config("unknown"), MotionLevel::Subtle);
    }

    #[test]
    fn spinner_uses_smooth_50ms_frames_with_12_step_cycle() {
        let first = MotionFrame::new(MotionLevel::Subtle, 0).running_icon();
        let second = MotionFrame::new(MotionLevel::Subtle, MotionFrame::TICK_MS).running_icon();
        let looped =
            MotionFrame::new(MotionLevel::Subtle, MotionFrame::TICK_MS * 12).running_icon();

        assert_ne!(first, second);
        assert_eq!(first, looped);
    }

    #[test]
    fn spinner_has_6_frame_forward_then_reverse_cycle() {
        let frames: Vec<&str> = (0..12)
            .map(|step| {
                MotionFrame::new(MotionLevel::Subtle, step * MotionFrame::TICK_MS).running_icon()
            })
            .collect();

        assert_eq!(frames[0], " ");
        assert_eq!(frames[1], "◐");
        assert_eq!(frames[2], "◓");
        assert_eq!(frames[3], "◑");
        assert_eq!(frames[4], "◒");
        assert_eq!(frames[5], "◕");
        assert_eq!(frames[6], "◕");
        assert_eq!(frames[7], "◒");
        assert_eq!(frames[8], "◑");
        assert_eq!(frames[9], "◓");
        assert_eq!(frames[10], "◐");
        assert_eq!(frames[11], " ");
    }

    #[test]
    fn cursor_blinks_without_width_shift() {
        let on = MotionFrame::new(MotionLevel::Subtle, 0).cursor();
        let off = MotionFrame::new(MotionLevel::Subtle, 500).cursor();

        assert_ne!(on, off);
        assert_eq!(on.chars().count(), off.chars().count());
    }

    #[test]
    fn off_mode_is_static() {
        let first = MotionFrame::new(MotionLevel::Off, 0);
        let later = MotionFrame::new(MotionLevel::Off, 10_000);

        assert_eq!(first.running_icon(), later.running_icon());
        assert_eq!(first.cursor(), later.cursor());
        assert_eq!(first.dots(), later.dots());
    }
}

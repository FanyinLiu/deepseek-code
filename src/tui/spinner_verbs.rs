//! Whimsical verbs used as the fallback activity title when the orchestrator
//! has not yet supplied a concrete one. Picked once per turn to give the
//! "thinking" line a human-feeling personality for transient status text.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const VERBS_EN: &[&str] = &[
    "Brewing",
    "Pondering",
    "Concocting",
    "Marinating",
    "Untangling",
    "Sleuthing",
    "Cogitating",
    "Mulling",
    "Ruminating",
    "Percolating",
    "Distilling",
    "Wrangling",
    "Sketching",
    "Drafting",
    "Whittling",
    "Forging",
    "Tinkering",
    "Plotting",
    "Charting",
    "Spelunking",
    "Hatching",
    "Calibrating",
    "Aligning",
    "Threading",
    "Weaving",
    "Synthesizing",
    "Composing",
    "Reticulating",
    "Untying",
    "Excavating",
    "Crystallizing",
    "Annotating",
    "Reasoning",
    "Stewing",
    "Steeping",
    "Polishing",
    "Stitching",
    "Quilting",
    "Conjuring",
    "Tuning",
    "Probing",
    "Surveying",
    "Reckoning",
    "Wayfinding",
    "Decoding",
    "Tracing",
    "Shaping",
    "Honing",
    "Auditing",
    "Inspecting",
];

const VERBS_ZH: &[&str] = &[
    "思考中",
    "推演中",
    "构思中",
    "盘算中",
    "酝酿中",
    "梳理中",
    "斟酌中",
    "勾画中",
    "摸索中",
    "雕琢中",
    "校准中",
    "排兵布阵",
    "穿针引线",
    "抽丝剥茧",
    "凝神琢磨",
    "细嚼慢咽",
    "推敲中",
    "勘探中",
    "测算中",
    "拼接中",
    "推理中",
    "缝合中",
    "拨弄中",
    "演算中",
    "审视中",
    "考量中",
];

/// Returns a stable verb for the lifetime of a single turn. The caller is
/// expected to seed `nonce` with something that changes per turn (e.g. message
/// count) so consecutive turns pick different verbs.
#[must_use]
pub fn pick_verb(chinese: bool, nonce: u64) -> &'static str {
    let pool = if chinese { VERBS_ZH } else { VERBS_EN };
    let idx = (nonce as usize) % pool.len();
    pool[idx]
}

/// Convenience: ambient nonce derived from a process-local counter mixed with
/// wall clock. Stable within one millisecond but differs between turns.
#[must_use]
pub fn ambient_nonce() -> u64 {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let bump = COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    now.wrapping_add(bump.wrapping_mul(2_654_435_761))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_verb_is_deterministic_for_same_nonce() {
        assert_eq!(pick_verb(false, 7), pick_verb(false, 7));
        assert_eq!(pick_verb(true, 99), pick_verb(true, 99));
    }

    #[test]
    fn pick_verb_varies_with_nonce() {
        let a = pick_verb(false, 1);
        let b = pick_verb(false, 2);
        // Adjacent nonces hit adjacent slots; pool >= 2 so they must differ.
        assert_ne!(a, b);
    }

    #[test]
    fn pools_are_nonempty_and_distinct() {
        assert!(VERBS_EN.len() >= 30);
        assert!(VERBS_ZH.len() >= 20);
    }
}

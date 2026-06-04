//! Behavioral eval for octocode's doom-loop reliability guard.
//!
//! `would_loop` decides whether the next tool call is a non-progressing
//! repetition (a single fixated call or a short oscillating cycle) that should
//! be skipped instead of executed. This harness scores it against a labeled
//! dataset of call sequences so "did the guard regress" is a number, not a
//! feeling. It runs with no model and no network, so it gates in CI.
//!
//! Run the scorecard locally with:
//!   cargo test --test reliability_eval_tests -- --nocapture

use deepseek_code::agent::tool_loop::would_loop;
use deepseek_code::deepseek::{ToolCall, ToolCallFunction, ToolCallRecord};
use serde::Deserialize;

/// Every reliability case is deterministic, so the guard must satisfy all of
/// them — a single miss is a real regression, not incidental tuning.
const ACCURACY_FLOOR: f64 = 1.0;

#[derive(Debug, Deserialize)]
struct Case {
    /// Prior tool calls in order, each `[name, arguments]`.
    history: Vec<(String, String)>,
    /// The call the model wants to make next, `[name, arguments]`.
    next: (String, String),
    /// Whether the guard should skip `next` as a non-progressing repetition.
    expect_skip: bool,
    notes: String,
}

fn record(name: &str, arguments: &str) -> ToolCallRecord {
    ToolCallRecord {
        id: "eval".to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
        result_summary: String::new(),
        exit_code: Some(0),
        duration_ms: 1,
        risk_level: "none".to_string(),
        approved: true,
        at: chrono::Utc::now(),
    }
}

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "eval".to_string(),
        call_type: "function".to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

fn load_cases() -> Vec<Case> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/evals/reliability_cases.jsonl");
    let data = std::fs::read_to_string(path).expect("read reliability eval cases");
    data.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse reliability eval case"))
        .collect()
}

fn passes(case: &Case) -> bool {
    let history: Vec<ToolCallRecord> = case.history.iter().map(|(n, a)| record(n, a)).collect();
    let next = call(&case.next.0, &case.next.1);
    would_loop(&history, &next) == case.expect_skip
}

#[test]
fn doom_loop_guard_meets_reliability_floor() {
    let cases = load_cases();
    assert!(
        cases.len() >= 10,
        "reliability eval should cover a meaningful spread of cases"
    );

    let mut passed = 0usize;
    let mut scorecard = String::from("\n=== doom-loop reliability eval scorecard ===\n");
    for case in &cases {
        if passes(case) {
            passed += 1;
            scorecard.push_str(&format!("PASS  {}\n", case.notes));
        } else {
            scorecard.push_str(&format!(
                "FAIL  {} (expected skip={})\n",
                case.notes, case.expect_skip
            ));
        }
    }

    let accuracy = passed as f64 / cases.len() as f64;
    scorecard.push_str(&format!(
        "cases={} passed={} accuracy={:.1}%\n",
        cases.len(),
        passed,
        accuracy * 100.0
    ));
    println!("{scorecard}");

    assert!(
        accuracy >= ACCURACY_FLOOR,
        "doom-loop reliability {:.1}% below floor {:.0}% — see scorecard above",
        accuracy * 100.0,
        ACCURACY_FLOOR * 100.0
    );
}

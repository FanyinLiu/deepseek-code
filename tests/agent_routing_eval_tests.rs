//! Behavioral eval for octocode's deterministic routing brain.
//!
//! `assess_with_rules` is what decides how risky/complex a task is, which in
//! turn drives mode and agent selection. This harness scores it against a
//! labeled dataset so "did routing get better or worse" is a number, not a
//! feeling. It runs with no model and no network, so it gates in CI.
//!
//! Run the scorecard locally with:
//!   cargo test --test agent_routing_eval_tests -- --nocapture

use deepseek_code::agent::router::rules::assess_with_rules;
use serde::Deserialize;

/// Minimum fraction of cases that must satisfy every labeled expectation.
/// A real regression in routing drops below this; incidental tuning does not.
const ACCURACY_FLOOR: f64 = 0.90;

#[derive(Debug, Deserialize)]
struct Case {
    input: String,
    #[serde(default)]
    expect_reason_codes: Vec<String>,
    #[serde(default)]
    expect_risk_flags: Vec<String>,
    #[serde(default)]
    expect_hard_trigger: Option<bool>,
    #[serde(default)]
    expect_min_score: Option<u32>,
    #[serde(default)]
    expect_max_score: Option<u32>,
    notes: String,
}

fn serialized(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn load_cases() -> Vec<Case> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/evals/agent_routing_cases.jsonl"
    );
    let data = std::fs::read_to_string(path).expect("read agent routing eval cases");
    data.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse routing eval case"))
        .collect()
}

fn evaluate(case: &Case) -> Vec<String> {
    let assessment = assess_with_rules(&case.input, None);
    let codes: Vec<String> = assessment.reason_codes.iter().map(serialized).collect();
    let flags: Vec<String> = assessment.risk_flags.iter().map(serialized).collect();
    let mut failures = Vec::new();

    for code in &case.expect_reason_codes {
        if !codes.contains(code) {
            failures.push(format!("missing reason {code} (got {codes:?})"));
        }
    }
    for flag in &case.expect_risk_flags {
        if !flags.contains(flag) {
            failures.push(format!("missing risk {flag} (got {flags:?})"));
        }
    }
    if let Some(expected) = case.expect_hard_trigger {
        if assessment.has_hard_trigger != expected {
            failures.push(format!(
                "hard_trigger {} != {expected}",
                assessment.has_hard_trigger
            ));
        }
    }
    if let Some(min) = case.expect_min_score {
        if assessment.score < min {
            failures.push(format!("score {} < min {min}", assessment.score));
        }
    }
    if let Some(max) = case.expect_max_score {
        if assessment.score > max {
            failures.push(format!("score {} > max {max}", assessment.score));
        }
    }
    failures
}

#[test]
fn agent_routing_rules_meet_accuracy_floor() {
    let cases = load_cases();
    assert!(
        cases.len() >= 18,
        "routing eval should cover a meaningful spread of cases"
    );

    let mut passed = 0usize;
    let mut scorecard = String::from("\n=== agent routing eval scorecard ===\n");
    for case in &cases {
        let failures = evaluate(case);
        if failures.is_empty() {
            passed += 1;
            scorecard.push_str(&format!("PASS  {}\n", case.notes));
        } else {
            scorecard.push_str(&format!(
                "FAIL  {} -> {}\n",
                case.notes,
                failures.join("; ")
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
        "routing accuracy {:.1}% below floor {:.0}% — see scorecard above",
        accuracy * 100.0,
        ACCURACY_FLOOR * 100.0
    );
}

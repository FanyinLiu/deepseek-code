//! AskUser — structured questions to the human.
//!
//! When the model needs a real decision (pick a library, resolve ambiguity,
//! choose a direction), it should call this instead of writing prose questions.
//!
//! This module provides:
//!   1. The argument shape the model fills in.
//!   2. Validation (max 4 questions, 2-4 options each, no preview with multi-select).
//!   3. A plain-text fallback renderer for the model's tool result.
//!
//! Real interactive prompting needs a TUI hook. Until that lands, the dispatch
//! arm uses `render_fallback` so the model at least sees its own questions
//! reflected back — the user can then type the answer in the next turn.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub const MAX_QUESTIONS: usize = 4;
pub const MAX_OPTIONS: usize = 4;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AskUserArgs {
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Question {
    pub question: String,
    pub header: String,
    #[serde(default)]
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

pub fn ask_user(args: AskUserArgs) -> Result<String> {
    validate(&args)?;
    Ok(render_fallback(&args))
}

/// Lightweight shape consumed by the orchestrator + event sink to surface a
/// pending interactive question in the TUI.
#[derive(Debug, Clone)]
pub struct PendingUserQuestion {
    pub title: String,
    pub options: Vec<String>,
    pub summary: String,
}

/// Parse the raw tool-call arguments into the orchestrator's lightweight
/// pending-question shape. Used by the agent loop to recognise `ask_user`
/// calls and surface them as TUI prompts instead of opaque tool results.
pub fn pending_question_from_value(value: &serde_json::Value) -> Result<PendingUserQuestion> {
    let args: AskUserArgs = serde_json::from_value(value.clone())
        .map_err(|e| anyhow!("invalid ask_user arguments: {e}"))?;
    validate(&args)?;
    let first = args
        .questions
        .first()
        .ok_or_else(|| anyhow!("ask_user produced no questions"))?;
    let options = first
        .options
        .iter()
        .map(|o| o.label.clone())
        .collect::<Vec<_>>();
    let summary = if args.questions.len() == 1 {
        first.question.clone()
    } else {
        format!(
            "{} (+{} more question{})",
            first.question,
            args.questions.len() - 1,
            if args.questions.len() == 2 { "" } else { "s" }
        )
    };
    Ok(PendingUserQuestion {
        title: first.header.clone(),
        options,
        summary,
    })
}

fn validate(args: &AskUserArgs) -> Result<()> {
    if args.questions.is_empty() {
        return Err(anyhow!("must ask at least one question"));
    }
    if args.questions.len() > MAX_QUESTIONS {
        return Err(anyhow!(
            "too many questions (max {MAX_QUESTIONS}, got {})",
            args.questions.len()
        ));
    }
    for (i, q) in args.questions.iter().enumerate() {
        if q.question.trim().is_empty() {
            return Err(anyhow!("question {i} has empty text"));
        }
        if q.header.trim().is_empty() {
            return Err(anyhow!("question {i} has empty header"));
        }
        if q.options.len() < 2 {
            return Err(anyhow!("question {i} needs at least 2 options"));
        }
        if q.options.len() > MAX_OPTIONS {
            return Err(anyhow!(
                "question {i} has too many options (max {MAX_OPTIONS})"
            ));
        }
        if q.multi_select && q.options.iter().any(|o| o.preview.is_some()) {
            return Err(anyhow!(
                "question {i}: previews only supported for single-select"
            ));
        }
    }
    Ok(())
}

fn render_fallback(args: &AskUserArgs) -> String {
    let mut out = String::from("Questions for the user:\n");
    for (qi, q) in args.questions.iter().enumerate() {
        out.push_str(&format!("\nQ{} [{}]: {}\n", qi + 1, q.header, q.question));
        if q.multi_select {
            out.push_str("(may pick one or more)\n");
        }
        for (oi, opt) in q.options.iter().enumerate() {
            out.push_str(&format!(
                "  {}. {} — {}\n",
                oi + 1,
                opt.label,
                opt.description
            ));
        }
        out.push_str("  *. Other (free text)\n");
    }
    out.push_str("\n(Waiting for the user's reply in the next turn. Do not assume an answer.)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(label: &str) -> QuestionOption {
        QuestionOption {
            label: label.into(),
            description: format!("{label} desc"),
            preview: None,
        }
    }

    #[test]
    fn rejects_too_few_options() {
        let r = ask_user(AskUserArgs {
            questions: vec![Question {
                question: "pick?".into(),
                header: "Pick".into(),
                multi_select: false,
                options: vec![opt("a")],
            }],
        });
        assert!(r.is_err());
    }

    #[test]
    fn rejects_preview_with_multi_select() {
        let mut o = opt("a");
        o.preview = Some("...".into());
        let r = ask_user(AskUserArgs {
            questions: vec![Question {
                question: "pick?".into(),
                header: "Pick".into(),
                multi_select: true,
                options: vec![o, opt("b")],
            }],
        });
        assert!(r.is_err());
    }

    #[test]
    fn happy_path() {
        let text = ask_user(AskUserArgs {
            questions: vec![Question {
                question: "Which library?".into(),
                header: "Library".into(),
                multi_select: false,
                options: vec![opt("serde"), opt("miniserde")],
            }],
        })
        .unwrap();
        assert!(text.contains("serde"));
        assert!(text.contains("Other"));
    }
}

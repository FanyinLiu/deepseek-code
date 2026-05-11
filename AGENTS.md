# Agent Preferences for DeepSeek-Code

## Workflow Style

**Default to multi-agent (parallel subagent) mode for all non-trivial work.**

- When a task involves touching more than 2 files, exploring unfamiliar modules, or fixing linter warnings across the codebase → **spawn parallel subagents** grouped by module/ concern.
- When a task is a single-line fix or a known-file edit → direct editing is fine.
- After any batch of parallel edits, always run `cargo check` and `cargo test` before declaring done.

## Context from Recent Work

- The project has a full multi-agent architecture (`SubagentExecutor`, `Supervisor`, `MessageBus`, `BackgroundQueue`).
- All 25 review findings from `review_report.md` have been fixed.
- `cargo clippy --all-targets --all-features` is currently clean (0 warnings).
- 84 tests pass; breaking the build or tests is unacceptable.

## Coding Conventions

- Follow existing Rust style in the repo.
- Prefer `expect("context")` over `unwrap()` in test code.
- When fixing clippy warnings, run `cargo clippy --fix` first, then hand-fix the rest in parallel.
- Keep changes minimal; don't refactor unrelated code while fixing a bug.

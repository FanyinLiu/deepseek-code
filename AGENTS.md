# Agent Preferences for Octocode

## Workflow Style

Default to multi-agent exploration for non-trivial work when the task spans multiple modules, unfamiliar code paths, or broad verification. Keep single-file fixes direct.

- When touching more than 2 files, first split read-only exploration by module or concern.
- After any batch of code edits, run `cargo check --all-targets --all-features` and `cargo test --all-features` before declaring done.
- Use `cargo clippy --all-targets --all-features -D warnings` for release-facing or broad refactor work.

## Current Product Shape

- Primary user command is `octo`.
- `octocode` is a compatibility binary only; do not make it the main documented entrypoint.
- Project config lives under `.octocode/`.
- Custom agents live in `.octocode/agents/*.md` with TOML frontmatter; validate them with `octo agent validate --all`.
- Mission dry-runs are project-local under `.octocode/missions/<mission-id>/` and should stay replayable from `events.jsonl`.
- The project has multi-agent infrastructure (`SubagentExecutor`, `Supervisor`, `MessageBus`, `BackgroundQueue`) plus TUI, MCP, policy, session, mission, repair, knowledge, and skill modules.

## Product Direction

- Position Octocode as a Rust, DeepSeek-native, Chinese/Windows-first local coding agent.
- Do not blindly copy upstream CLIs. Prefer simpler local architecture unless a larger abstraction removes real complexity.
- Keep Droid-style TUI behavior quiet and stable: no distracting blinking, no separate welcome page, API setup and workbench live in one surface.
- Language selection must affect visible command descriptions, help text, settings, approval, planning, context, and error copy wherever practical.
- Context numbers should be derived from provider/model capability and local budget; do not fake a large context window by displaying an arbitrary configured value.

## Coding Conventions

- Follow existing Rust style in the repo.
- Prefer `expect("context")` over `unwrap()` in test code.
- Keep changes minimal and avoid unrelated refactors.
- For CLI product-surface changes, add or update binary integration tests using `env!("CARGO_BIN_EXE_octo")` for primary command behavior.
- Preserve compatibility tests for `env!("CARGO_BIN_EXE_octocode")` where alias behavior matters.
- After touching mission storage, run `cargo test --test mission_store_tests --all-features` and `cargo test --test mission_cli_tests --all-features`.

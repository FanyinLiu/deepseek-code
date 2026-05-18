# Agent Parity Audit

Date: 2026-05-09

This audit lists the current terminal-agent parity items one by one, based on
static code inspection plus the current local verification gates.

## Verification Baseline

- `cargo fmt --all --check`: pass
- `cargo check --all-targets --all-features`: pass
- `cargo test --all-features`: pass, 221 lib tests plus integration suites
- `cargo clippy --all-targets --all-features -- -D warnings`: pass

## Status Legend

- Complete: implemented and covered enough for the current MVP.
- Partial: implemented at least once, but not yet at target parity.
- Missing: no meaningful implementation found.

## Item-by-Item Audit

| Area | Item | Status | Evidence | Gap | Next Step |
|---|---|---|---|---|---|
| Tools | `apply_patch` dispatch | Complete-ish | Tool schema in `src/deepseek/tools.rs`; dispatch in `src/tools/dispatch.rs`; path parsing in `src/workspace/apply.rs`; approval tests in `src/policy/approvals.rs`. | Approval now blocks unparseable/protected patch targets before execution; remaining gap is dependence on `git apply` and no explicit patch size/timeout limit. | Add patch size and execution timeout limits; consider a non-git fallback for simple patches. |
| Tools | `write_file` new files | Complete-ish | `src/tools/write_file.rs`; workspace path checks in `src/workspace/apply.rs`; coverage in `tests/tool_correctness_tests.rs`. | New files work and dispatch no longer auto-runs tests; orchestrator self-verification is skipped when command approval is required. | Add an explicit approved verification flow, so users can approve one post-edit check cleanly. |
| Tools | `run_command` timeout | Complete | Timeout kill path in `src/tools/run_command.rs`; schema in `src/deepseek/tools.rs`; dispatch config in `src/tools/dispatch.rs`; tests in `tests/tool_correctness_tests.rs`. | None for MVP. | Later: per-command profiles and UI display of the chosen timeout. |
| Tools | `fetch_url` guard | Complete-ish | Private/local checks and response limit in `src/tools/fetch_url.rs`; tests in `tests/tool_correctness_tests.rs`. | DNS rebinding/TOCTOU risk remains because validation and actual reqwest connect are separate. | Long term: custom resolver or network-layer SSRF guard. |
| Startup | API key onboarding | Partial | CLI prompt in `src/cli/login.rs`; keyring/project fallback in `src/storage/keyring.rs`; explicit TUI `ApiKeyState` in `src/tui/app.rs`; masked entry in `src/tui/input.rs`. | The TUI no longer loops back to missing after a successful save, but it still constructs a client with an empty key before onboarding; project fallback is plaintext local config; redaction helper is not broadly wired. | Delay client creation until key exists; add optional doctor-lite validation; wire redaction into logs/transcripts. |
| Startup | `cargo run` default | Partial | `default-run = "octocode"` in `Cargo.toml`. | Bare run enters CLI welcome/chat, not Ratatui TUI. Product default is not yet decided. | Decide whether default should be TUI, then add smoke tests for bare `cargo run` and `octocode tui`. |
| Commands | Built-in slash commands | Partial | Registry in `src/commands/mod.rs`; TUI prefix hints and Tab completion in `src/tui/app.rs`; local `/tasks`; forward tests for `/run`, `/ask`, `/plan`. | No custom command file discovery; `# memory` is only a hint; `/run` etc. are forwarded rather than first-class command flows. | Add `.octocode/commands` and user command loading; add a full command palette and registry snapshot tests. |
| Permissions | Approval model | Partial | Policy in `src/policy/approvals.rs`; ToolLoop approval in `src/agent/tool_loop.rs`; TUI once/session/deny in `src/tui/app.rs`. | No unified first-class modes; `git_add` is now marked as git mutation, but subagent `AcceptEdits` still asks in some paths and verification needs a first-class approval UI. | Build one permission state machine for default/accept-edits/plan/auto/deny/bypass; route verification through an explicit approval flow. |
| TUI | Welcome UI | Partial | Wide/compact welcome in `src/tui/welcome.rs`; app layout in `src/tui/layout.rs`; preview snapshots in `src/tui/app.rs`. | Product identity exists, but mascot, compact/narrow polish, and command discoverability still need iteration. | Add 80x24 render smoke and keep refining welcome/workbench visual language. |
| TUI | Typing/input | Partial | Input rendering in `src/tui/input.rs`; key handling in `src/tui/app.rs`; repeat-key, shell mode, multiline, slash Tab, `@file` Tab, and arrow-history tests. | There is still no full command palette or searchable history; mention completion is intentionally lightweight. | Add overlay command palette and Ctrl-R fuzzy history search. |
| TUI | Dynamic preview | Partial | Hidden `preview-tui` in `src/cli_entry.rs`; `PreviewSnapshotScenario` in `src/tui/app.rs`. | It is one-shot text rendering, not fixture replay or live preview. | Add event playback fixtures for streaming, plan, approval, and subagent flows. |
| Plan | Plan mode | Partial | Plan generation/review/options in `src/agent/orchestrator.rs`; tracker in `src/tui/plan_tracker.rs`. | No refine/edit plan option; read-only plan behavior is partly prompt-enforced; high-risk split option is not a real decomposition path. | Add refine plan, explicit read-only plan mode, and real decomposition for high-risk plans. |
| Plan | Plan UI | Partial | `PlanStarted` and step updates in `src/agent/orchestrator.rs`; typed tracker labels in `src/tui/plan_tracker.rs`; rendering in `src/tui/app.rs`. | Tracker is clearer but still compact; transcript and plan lifecycle can be clearer. | Add plan history section and preserve selected execution mode in transcript. |
| Subagents | Execution model | Partial | Supervisor and executor in `src/agent/supervisor.rs` and `src/agent/subagent/executor.rs`; no nested subagents; TUI `/tasks` command in `src/commands/mod.rs`. | Custom agent frontmatter is not yet normalized; background task IDs are not linked to live cards. | Add YAML frontmatter compatibility, permission inheritance, and background-task/card linkage. |
| Subagents | Streaming/card UI | Partial | `SubagentDelta` in `src/agent/orchestrator.rs`; executor delta bridge in `src/agent/subagent/executor.rs`; cards in `src/tui/subagent_cards.rs`. | Reasoning/content are collapsed into one card update string. | Add structured subagent delta kind and show reasoning as summarized phase, not raw content. |
| Thinking | Reasoning display | Partial | Default hidden reasoning in `src/tui/app.rs`; toggle with `t`; transcript marker in `src/tui/transcript_view.rs`. | No summarized thinking channel; raw opt-in exists but is not polished. | Add concise live thinking summaries and keep raw reasoning opt-in only. |
| Streaming | Main streaming | Partial | SSE in `src/deepseek/client.rs`; chunk callbacks in `src/agent/orchestrator.rs`; TUI buffer in `src/tui/app.rs`. | Esc/interrupt does not abort the active API call; final visual transition can be abrupt. | Add cancellation token/abort support and better final stream-to-transcript transition. |
| MCP | MCP integration | Partial | Stdio client in `src/mcp/client.rs`; registry filters in `src/mcp/registry.rs`; orchestrator calls MCP tools. | No HTTP/SSE transport, auth flow, dynamic MCP prompts; old `tools/mcp_wrapper.rs` remains stub-like. | Remove/merge old wrapper, add transport enum, HTTP/SSE clients, auth placeholders, prompts list/get. |
| Hooks | Hooks | Partial | Config schema in `src/storage/config.rs`. | No HookRunner, prompt submit, pre/post tool, stop, or subagent stop execution path. | Add HookEvent/HookRunner and wire pre/post tool plus stop first. |
| Skills | Skills | Missing | Welcome skills are hard-coded tool display in `src/tui/welcome.rs`; no `SKILL.md` loader found. | No skills directory discovery, metadata, preload, or prompt injection. | Add `.octocode/skills/<name>/SKILL.md` loader and prompt injection strategy. |
| Session | Session resume/export | Complete-ish | Session store and transcript export in `src/storage/sessions.rs` and `src/storage/transcripts.rs`; CLI in `src/cli/resume.rs`. | TUI resume/export integration is thin. | Add TUI session picker and richer export/status commands. |
| Memory | Project/user memory | Partial | Project guidance injection in `src/agent/prompt_builder.rs`; `/memory` in `src/commands/mod.rs`. | Mostly read-only listing/injection; no interactive edit/add/import or precedence diagnostics. | Add `/memory add/edit/import` and precedence tests. |
| Distribution | Release artifacts | Partial | Release workflow in `.github/workflows/ci.yml`; dual binaries in `Cargo.toml`. | No real installers, checksums/signing, shell completions, Scoop/Homebrew manifests. | Add archives/checksums first, then completions and package manifests. |
| CI | CI matrix | Partial | Linux/macOS/Windows matrix in `.github/workflows/ci.yml`. | CI lacks explicit `cargo check --all-targets --all-features`; no TUI/Windows smoke or release artifact smoke. | Add check gate and smoke tests. |
| Docs | User/developer docs | Partial | README and `docs/agent_parity_test_plan.md`. | MCP/hooks/skills/memory/distribution docs are missing or too thin; install docs contain placeholders. | Add dedicated docs for MCP, hooks, memory, skills, and distribution. |

## Highest Priority Fix Order

1. Permissions consistency:
   - Done: parse `apply_patch` affected paths before approval.
   - Done: expose/default `run_command.timeout_seconds`.
   - Done: remove low-level implicit verification and skip self-verification when command approval is required.
   - Next: add explicit approved verification flow and patch size/timeout limits.

2. TUI interaction completion:
   - Done: implement `!` shell mode through approved `run_command`.
   - Done: add slash Tab completion and lightweight `@` file mention completion.
   - Next: add command palette for `/` and Ctrl-R fuzzy history.

3. Plan and subagent lifecycle:
   - Done: add TUI `/tasks` and typed plan tracker labels.
   - Add refine/edit plan option.
   - Structure subagent content vs reasoning deltas.

4. Ecosystem parity:
   - Add skills loader.
   - Wire hooks execution.
   - Add HTTP/SSE MCP transports.

5. Distribution polish:
   - Add CI `cargo check` gate.
   - Add release smoke tests, checksums, completions, and real install docs.

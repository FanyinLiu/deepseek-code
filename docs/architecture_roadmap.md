# Octocode Architecture Roadmap

This roadmap is the authoritative plan for taking Octocode from its current shape to a
local-first team-agent workbench. It is intentionally architecture-first: every
phase lands a boundary before any UX polish or feature growth.

## Companion Documents

- [`architecture_map.md`](architecture_map.md) — current module layout and the
  subsystem owner for every boundary.
- [`competitor_parity_matrix.md`](competitor_parity_matrix.md) — what Octocode should
  absorb (and avoid) from Codex, Gemini, Claude Code, Kimi, Droid, Qwen.
- [`runtime_boundaries.md`](runtime_boundaries.md) — the canonical names for
  RuntimeKernel, Team Runtime, Tool Runtime, plan/mission/task, TUI workbench.

The roadmap below references these documents; do not duplicate their content.

## Product Position

Octocode is not a generic chat CLI. The target form is a **local-first team-agent
workbench**:

- A clear coordinator agent plans, dispatches, and merges work.
- Specialized subagents cover explore, review, plan, work, verify, test.
- One shared runtime kernel powers CLI, TUI, mission resume, and saved tasks.
- Tools, policy, audit, and approvals run through a single path regardless of
  caller.

The roadmap protects this position. Every phase ships an architectural boundary,
not a feature flag.

## Target Architecture

| Layer | Purpose | Canonical types |
|---|---|---|
| `RuntimeKernel` | Shared services for every surface | config, session, audit, policy, tools, model |
| Turn / Plan / Team runtimes | Per-purpose controllers above the kernel | `TurnController`, `PlanRuntime`, `TeamRuntime` |
| Tool runtime | One dispatch + approval + audit path | `ToolCall → PolicyDecision → ApprovalRequest → ToolBackend → ToolResult → SessionEvent` |
| Team runtime | Coordinator + role-specialized agents | `TeamPlanDraft → TeamPlan → TeamRun → TeamTask → AgentRunState` |
| Plan / mission / task | Distinct lifecycle, no overlap | read-only plan, replayable mission, triggerable task |
| TUI workbench | Renders runtime events only | transcript, team panel, approval panel, status/footer, terminal lifecycle |
| Storage / audit | Facts vs summaries | `events.jsonl` as facts, Markdown artifacts as summaries |

## Phases

### Phase 0 — Freeze and map (in progress)

Goal: document the current state and the boundary names before any refactor.

- `docs/architecture_map.md` — done.
- `docs/competitor_parity_matrix.md` — done.
- `docs/runtime_boundaries.md` — done.
- `docs/architecture_roadmap.md` — this file.
- Scaffolding lands as types only: `src/runtime/mod.rs` (RuntimeKernel
  descriptor) and `src/agent/team.rs` (canonical team types). Existing
  orchestrator and swarm executor stay live; `SwarmPlan::team_plan` already
  maps to the new shape.

Exit criteria: all four documents committed; new modules compile; no behavior
change for existing CLI/TUI flows.

### Phase 1 — Runtime boundary split

Goal: peel controllers out of the orchestrator without changing user-visible
behavior or `AgentEvent` shape.

- Extract `TurnController` (per-turn routing + context + streaming).
- Extract `ToolRuntime` wrapping current dispatch (policy + approval +
  backend + result + change set).
- Extract `TeamRuntime` driving `TeamPlan → TeamRun → TeamTask` while the
  legacy swarm path continues to emit `Swarm*` events.
- Extract `PlanRuntime` (read-only proposal + validation strategy).
- Extract `EventRecorder` that owns the durable `events.jsonl` writer.

Exit criteria: CLI, TUI, and mission code paths construct a `RuntimeKernel`
and ask it for controllers; old commands behave identically; `cargo test`
green; new typed events available internally but not yet required by UI.

### Phase 2 — Team agent formalization

Goal: make team execution a first-class entrypoint with role boundaries.

- `run_subagent` covers single-agent only.
- New `run_team` / `run_swarm` entrypoint is explicit, not a flag flip.
- Only `Worker` produces pending patches; the coordinator's tool runtime is
  the only path that lands changes.
- Coordinator-merged Markdown artifact per team run: goal, assignment,
  confirmations, change set, validation, outcome.

Exit criteria: complex tasks fan out into complementary roles without
duplication; read-only tasks never spawn a worker; modify tasks only land
patches through the coordinator.

### Phase 3 — TUI workbench rebuild

Goal: stop the workbench from owning business logic.

- Split `app.rs` into input, transcript, team panel, approval panel,
  status/footer, terminal lifecycle modules.
- Default to a classic terminal: native scrollback, copy/paste, no
  full-screen takeover. Fullscreen stays available as an opt-in mode.
- Token state animates in one fixed position; input vs output token phases
  are sequential, never racing for space.
- Team panel renders one row per agent; success collapses, failure expands
  structured details.
- Approval popup states source (main / subagent / MCP / task), risk class,
  command or file summary, and the keybindings.

Exit criteria: iTerm2, Windows Terminal, VS Code Terminal all show stable
transcript scroll, copy, paste, exit-restore, token animation, approval
popup.

### Phase 4 — Plan / Mission / Task semantics

Goal: prevent the three lifecycles from collapsing into one ambiguous flow.

- `plan` stays read-only and returns a proposal + validation strategy.
- `mission` records a resumable long run with facts in `events.jsonl` and a
  Markdown summary artifact.
- `task` is the saved local trigger definition; running a task may start a
  mission but is not the run.
- Resume never re-executes side-effectful commands automatically; it shows
  state and the next step instead.

Exit criteria: each verb is reachable by exactly one CLI surface; mission
resume shows state without re-running tools; saved task triggers always go
through mission for replay.

### Phase 5 — Policy, config, output protocol

Goal: make the safety and integration surface explicit.

- Policy profiles: read-only, workspace-write, test/build, network/git/deploy.
- Config precedence: `user > project trusted > env > cli flag`.
- Stable `--json` / `--stream-json` output, exit codes, debug log path.
- MCP, shell, file write, and network all share the same approval + audit
  path.

Exit criteria: a single matrix lists every tool source × policy decision;
output flags are documented and exercised by tests; debug log paths are
stable across surfaces.

## Review Loop

After every non-trivial subtask, run three read-only reviewers:

- Architecture reviewer — boundary respect, no new dispatch paths.
- Competitor parity reviewer — does the change align with the absorb/avoid
  list in the parity matrix?
- Tests / safety reviewer — `cargo fmt --check`, `git diff --check`,
  targeted tests; full `cargo test` + `cargo clippy --all-targets
  --all-features` at the end of a phase.

Reviewer output goes to artifacts, not the main transcript. The coordinator
merges only conclusions and blocking findings into the user-facing summary.

## Out of Scope

- Cloud-managed agents and remote sandboxes.
- Background daemons or scheduled task execution beyond local manual runs.
- New slash or CLI surface area before the runtime boundaries land.
- Adopting closed-source behavior that is not documented by official sources.

## Working Agreements

- Keep `AgentEvent` and existing CLI command names compatible during the
  transition; add new typed events alongside, do not rename.
- No new tool execution path. Wrap the existing dispatch until every caller
  reaches `ToolRuntime`.
- Raw model reasoning, raw subagent errors, protocol noise stay in
  `events.jsonl`; they never reach the main TUI transcript.
- Uncommitted runtime artifacts (`logs/`, `.octocode/missions/`,
  `TASK.md`) are gitignored and stay local to this project.

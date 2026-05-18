# Runtime Boundaries

Octocode is moving toward one local-first runtime kernel shared by CLI, TUI, saved
tasks, and missions. This document defines the decision boundaries for the
implementation.

## RuntimeKernel

`RuntimeKernel` names six shared services:

- `config`: resolved user/project/env/CLI configuration.
- `session`: session snapshots, transcripts, and resume metadata.
- `audit`: durable `events.jsonl` and Markdown artifacts.
- `policy`: path, command, network, approval, and sandbox decisions.
- `tools`: tool schema, backend dispatch, summaries, changed files.
- `model`: DeepSeek client, streaming, token accounting, and lanes.

The kernel is currently a descriptor in `src/runtime`; follow-up phases should
make CLI, TUI, mission, and task runners construct and pass it instead of
assembling services independently.

## Team Runtime

Canonical team objects live in `src/agent/team.rs`:

- `TeamPlanDraft`: untrusted model/local draft before validation.
- `TeamPlan`: validated plan with milestones, acceptance, tasks, commands, risks.
- `TeamTask`: one role-owned assignment with focus files and write mode.
- `TeamRun`: durable run state.
- `AgentRole` and `AgentRunState`: stable UI/event labels.

The existing swarm executor should map to these types while preserving old
`Swarm*` events until TUI and CLI consumers are migrated.

## Tool Runtime

Every caller must converge on:

`ToolCall -> PolicyDecision -> ApprovalRequest -> ToolBackend -> ToolResult -> SessionEvent`

The current `LocalToolBackend` wraps existing dispatch; future MCP/subagent/task
execution should use the same result shape and changed-file detection.

## Plan / Mission / Task

- `plan`: read-only proposal and validation strategy.
- `mission`: recoverable long-running run with facts in event logs.
- `task`: saved local trigger definition; it may run a mission but is not the run itself.

This separation prevents the UI from asking for plan confirmation when the user
asked for an explicit team run, and prevents background tasks from silently
replaying side-effectful commands.

## TUI Workbench

The workbench should render runtime events, not own business logic:

- transcript: user-visible messages and structured summaries.
- team panel: one row per agent, details expanded only on failure/block.
- approval panel: source, risk, command/file summary, and action keys.
- status/footer: model, mode, permission, token direction, context, tool state.
- terminal lifecycle: classic by default, fullscreen optional.

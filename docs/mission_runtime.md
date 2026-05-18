# Mission Dry-Run Runtime

`octocode mission` records a local dry-run plan for long-running work. The current
runtime is intentionally minimal: it does not execute model calls or edit files.
It creates a durable mission record, writes a local-rule plan, appends events,
and can replay those events into state.

## Commands

```bash
octocode mission new "refactor src/agent safely" --dry-run
octocode mission new "refactor src/agent safely" --dry-run --json
octocode mission status latest
octocode mission status <mission-id> --json
octocode mission inspect latest
octocode mission inspect latest --json
octocode mission inspect latest --events
octocode mission replay latest
octocode mission list
octocode mission list --json
```

Mission targets accept `latest`, a full mission id, or a unique id prefix.

## Store Layout

Mission records are project-local:

```text
.octocode/missions/
  index.json
  <mission-id>/
    mission.json
    events.jsonl
    state.json
    plan.json
```

`mission.json` stores the goal, recommended mode, dry-run flag, project root,
and timestamps.

`plan.json` stores:

- goal
- recommended mode
- DAG-ish steps with dependencies
- suggested agents
- expected validation commands
- risk notes

`state.json` stores the latest mission state.

`events.jsonl` stores append-only events:

```text
mission_created
plan_generated
mission_completed
mission_failed
```

The event loader keeps valid prior events if only the final non-empty JSONL line
is malformed. This protects a mission replay from losing the full timeline after
an interrupted append.

## Local Planning Rules

Dry-run planning uses local heuristics:

- review or safety work over multiple files recommends `swarm`
- explanation or architecture analysis recommends `agent-run`
- broad refactor plus tests recommends `mission-dry-run`
- tiny wording/typo work recommends `direct`
- everything else starts in `plan`

Validation commands default to `cargo check --all-targets --all-features`, then
add `cargo fmt --all --check`, `cargo test`, or
`cargo clippy --all-targets --all-features` when the task wording suggests
formatting, tests, or security-sensitive work.

## Intended Use

Use mission dry-runs before broad changes to preserve a durable plan and event
timeline:

```bash
octocode mission new "review src/agent and storage for release safety" --dry-run
octocode mission inspect latest --events
octocode mission replay latest
```

This is a foundation for future long-running execution. The current release only
records and replays dry-run state.

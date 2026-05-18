# Octocode Knowledge / Failure Memory / Skill Theory

Status: local theory draft  
Scope: project knowledge, failure memory, risk map, skill library  
Purpose: define how Octocode converts one-off task experience into reusable long-term capability.

## 1. Core Position

`knowledge`, `failure-memory`, and `skill` are the middle layers that move Octocode from a normal coding agent toward a self-evolving agent.

They solve this problem:

```text
The model may be smart every time, but the system starts from zero every time.
```

Octocode must preserve task history as assets for the next iteration.

## 2. Three Long-Term Assets

Octocode has three asset classes:

| Asset | Role | Source | Consumers |
|---|---|---|---|
| Project Knowledge | understand the current project | scans, user rules, history | planner / repair / judge |
| Failure Memory | avoid repeated failures | failed runs, rollbacks, test failures | repair / evolve / judge |
| Skill Library | reuse successful capability | successful runs, manual curation, paper mechanisms | planner / repair / evolve |

Relationship:

```text
Project Knowledge tells the agent the environment.
Failure Memory helps it avoid old mistakes.
Skill Library lets it reuse successful methods.
```

## 3. Project Knowledge Theory

Project Knowledge is the project-level environment map.

It should answer:

- What is this project?
- What are the main commands?
- Where are the core modules?
- Which files are high risk?
- Which tests validate which capabilities?
- How are provider, TUI, CLI, agent, and mission modules connected?
- What are the user's stable preferences?
- Which operations are costly or state-breaking?

Suggested files:

```text
.octocode/knowledge/project.md
.octocode/knowledge/modules.json
.octocode/knowledge/commands.json
.octocode/knowledge/validation.json
```

### Suggested project.md structure

```text
# Project Knowledge

## Product Identity
## Architecture Map
## Core Modules
## CLI Commands
## Provider System
## TUI System
## Agent System
## Mission System
## Validation Commands
## High-Risk Areas
## User Preferences
## Known Constraints
## Last Updated
```

## 4. Risk Map Theory

Risk Map is the most important machine-readable part of Project Knowledge.

Suggested file:

```text
.octocode/knowledge/risk-map.json
```

Suggested structure:

```json
{
  "version": 1,
  "paths": [
    {
      "pattern": "src/policy/**",
      "risk_level": "high",
      "reason": "approval and safety policy",
      "required_gates": ["cargo_check", "full_tests", "security_scan"],
      "auto_apply_allowed": false
    }
  ]
}
```

Risk levels:

| Level | Meaning | Policy |
|---|---|---|
| low | docs, comments, non-core prompts | low-cost validation |
| medium | normal business logic | check and targeted tests |
| high | safety, permissions, secrets, command execution, core orchestration | full gates, no auto-apply |
| blocked | bypass safety, hide failures, delete tests | reject immediately |

Risk Map sources:

- Static rules.
- AGENTS.md.
- Project structure.
- User preferences.
- Historical failures.
- Rollback records.
- Judge disagreement records.

## 5. Failure Memory Theory

Failure Memory is Octocode's negative experience system.

It is not an error log. It is searchable failure knowledge.

Suggested file:

```text
.octocode/knowledge/failure-memory.jsonl
```

Suggested record:

```json
{
  "id": "failure_...",
  "created_at": "...",
  "source": "repair|evolve|test|manual",
  "proposal_id": "...",
  "run_id": "...",
  "symptom": "...",
  "root_cause": "...",
  "touched_files": ["..."],
  "failed_gate": "cargo_check",
  "validation_excerpt": "...",
  "failed_patch_hash": "...",
  "avoid_pattern": "...",
  "lesson": "...",
  "tags": ["provider", "cli", "risk:high"]
}
```

### Failure Memory Usage

During `propose`:

- Find similar historical issues.
- Warn planner not to repeat old approaches.

During `localization`:

- Prioritize historically relevant files.

During `patch`:

- Block repeated failed patch patterns.

During `judge`:

- Lower confidence for similar failed diffs.

During `skill`:

- Populate `when_not_to_use` and `failure_modes`.

## 6. Skill Theory

A Skill is a capability package made from successful experience.

It is not just a prompt or a document summary.

A skill must answer:

- When should it be used?
- When should it not be used?
- What context does it require?
- What are the execution steps?
- How is it validated?
- What are common failures?
- What is its historical performance?

Suggested directory:

```text
.octocode/skills/<skill-id>/
  SKILL.md
  metadata.json
  examples/
  tests/
  traces.jsonl
```

### Suggested SKILL.md structure

```text
# Skill Name

## Purpose
## When to Use
## When Not to Use
## Required Context
## Steps
## Validation
## Examples
## Failure Modes
## Related Files
```

### Suggested metadata.json structure

```json
{
  "id": "rust-cli-command-wiring",
  "version": 1,
  "created_from_run": "run_...",
  "status": "draft|active|deprecated",
  "success_count": 0,
  "failure_count": 0,
  "last_used_at": null,
  "tags": ["rust", "cli"]
}
```

## 7. Skill Promotion Theory

Not every success should become a skill.

A run is suitable for promotion if:

- The problem type repeats.
- The steps are describable.
- Validation is stable.
- Success was not accidental.
- Failure boundaries can be stated.
- Reuse value exceeds maintenance cost.

Not suitable:

- One-off business changes.
- Success that depends on special context.
- No validation.
- High risk without enough failure samples.
- Overly abstract steps.

Promotion flow:

```text
successful run
-> extract repeated pattern
-> draft skill
-> attach validation
-> mark draft
-> use in future run
-> update traces
-> activate or deprecate
```

## 8. Knowledge Update Strategy

Knowledge should not be fully rewritten every time.

Recommended strategy:

- Append first.
- Periodically compact.
- High-risk changes require explicit notes.
- Stale entries should be marked, not silently deleted.
- Generated knowledge should include source trace.

States:

| State | Meaning |
|---|---|
| active | currently valid |
| stale | may be outdated |
| disputed | judge or test results conflict |
| deprecated | should not be used |

## 9. Retrieval Theory

The planner should not read all knowledge and all skills.

Recommended retrieval order:

1. Match task keywords to tags.
2. Match touched files to risk map.
3. Match error text to failure memory.
4. Match module to project knowledge.
5. Match historical success to skills.
6. Put only top-k summaries into context.

Default context bundle:

```text
project summary
risk flags
similar failures top 3
candidate skills top 3
validation hints
```

## 10. Avoiding Knowledge Pollution

Long-term memory can be polluted.

Risks:

- Old knowledge becomes stale.
- A single failure is overgeneralized.
- Bad skills are repeatedly reused.
- User preferences are recorded incorrectly.
- Provider behavior changes and invalidates old experience.

Controls:

- Every knowledge item has a source.
- Every skill has success/failure counts.
- Failure memory has tags and similarity limits.
- Stale state is explicit.
- Judges can dispute knowledge.
- Users can delete or disable skills.

## 11. Relationship to Repair

Repair Agent should consume:

- Project knowledge.
- Risk map.
- Failure memory.
- Skills.

Repair Agent should produce:

- Repair trace.
- Failure memory.
- Skill candidate.
- Risk-map incident.
- Project knowledge update suggestion.

Flow:

```text
repair propose
-> retrieve knowledge/failures/skills
-> repair run
-> validation
-> write trace
-> update failure-memory or skill candidate
```

## 12. Relationship to Evolve

Evolve should consume:

- Risk map.
- Failure memory.
- Repair skills.
- Project knowledge.

Evolve should produce:

- Archive records.
- Lineage records.
- New core repair skills.
- Risk incidents.
- Rollback lessons.

Principle:

```text
evolve should not bypass knowledge/failure-memory/skill; it should self-modify based on them.
```

## 13. Recommended First Version Scope

First version should include only:

```text
.octocode/knowledge/project.md
.octocode/knowledge/risk-map.json
.octocode/knowledge/failure-memory.jsonl
.octocode/skills/<skill-id>/SKILL.md skeleton
```

Commands:

```bash
octocode knowledge update
octocode knowledge show risk-map
octocode knowledge show failures
octocode skill list
octocode skill show <skill-id>
```

Defer:

- Automatic skill activation.
- Vector database.
- Large-scale memory compaction.
- Automatic deletion of old knowledge.
- Cross-project shared skill marketplace.

## 14. Acceptance Criteria

First version acceptance:

- It can generate a project knowledge file.
- It can generate a risk map.
- repair/evolve failures can append to failure memory.
- It can list and show skills.
- Planner can read top-k failure/skill summaries.
- Knowledge items have source and status.
- Old memory is not silently deleted.

## 15. Final Judgment

The Knowledge / Failure Memory / Skill theory is complete enough for a first implementation.

Recommended implementation order:

```text
risk-map -> failure-memory -> project.md -> skill skeleton -> retrieval bundle
```

This moves Octocode from "thinking from scratch every time" to accumulating engineering experience over time.

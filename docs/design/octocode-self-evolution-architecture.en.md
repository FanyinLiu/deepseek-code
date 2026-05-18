# Octocode Self-Evolution Architecture

Status: local design draft  
Scope: Octocode CLI / agent framework  
Goal: convert the self-evolving agent research direction into an implementable, auditable, and rollback-safe product architecture.

## 1. Core Position

Octocode should not start by pursuing AGI consciousness or unconstrained recursive self-modification. It should first implement an engineering-grade self-evolution system:

```text
reliable code repair -> experience and skill accumulation -> sandboxed core self-modification -> benchmarked selection and rollback
```

This is closer to a practical combination of DGM, SICA, HGM, Voyager, Agentless, AutoCodeRover, and Agent-as-a-Judge than to any single paper.

## 2. Goals

Octocode self-evolution should satisfy these goals:

- It can modify both user projects and Octocode's own source code.
- Every modification has a proposal, patch, run, gate result, apply record, and rollback path.
- Models may propose and review changes, but they cannot be the only source of correctness.
- Low-risk improvements may be automated; high-risk core changes require stronger gates.
- Successful patterns become reusable skills, rules, and project knowledge.
- Evaluation should be token-light by default: use diffs, tests, static rules, and historical failure patterns before full-context LLM review.
- Multiple models, roles, and judges should reduce same-model self-confirmation.

## 3. Non-Goals

This phase does not attempt to:

- Train or fine-tune foundation model weights.
- Let agents bypass permissions, approvals, sandboxes, or test gates.
- Treat benchmark score as the only objective.
- Let a self-modification overwrite the main workspace without a checkpoint.
- Depend on opaque, unauditable evolution as a core product capability.

## 4. Three-Layer Architecture

### Layer 1: Engineering Repair Baseline

References: Agentless, AutoCodeRover, SWE-agent.

The first priority is to make Octocode a stable repository repair agent:

```text
issue input -> locate relevant code -> generate patch -> verify -> report -> apply or rollback
```

Key capabilities:

- Structured code search.
- AST/symbol-level localization.
- Test failure attribution.
- Minimal patch generation.
- Cost, latency, and token accounting.
- Replayable repair runs.

Suggested commands:

```bash
octocode repair propose "fix failing provider switch"
octocode repair run <proposal-id>
octocode repair report <run-id>
octocode repair apply <run-id>
```

### Layer 2: Experience and Skill Evolution

References: Voyager, Reflexion, Self-Refine, Self-Debug, World Knowledge.

The goal is to make Octocode learn from successful and failed work instead of starting from scratch every time.

Reusable assets:

- `skills`: callable procedures such as Rust CLI refactor, TUI snapshot audit, provider debugging.
- `rules`: durable constraints such as high-risk paths that cannot be auto-applied.
- `project knowledge`: architecture map, critical modules, common failure modes.
- `failure memory`: failed patches, causes, and trigger patterns.
- `bench traces`: validation results, duration, token usage, and cost.

Suggested commands:

```bash
octocode skill list
octocode skill add <run-id>
octocode skill test <skill-id>
octocode knowledge update
octocode knowledge show risk-map
```

### Layer 3: Core Code Self-Evolution

References: DGM, SICA, HGM, Gödel Agent.

The goal is to let Octocode improve its own core implementation under controlled conditions.

Core loop:

```text
select target -> create proposal -> generate candidate patch -> sandbox verification -> multi-judge evaluation -> archive -> optional apply -> rollback if needed
```

Existing commands can be extended:

```bash
octocode evolve inspect
octocode evolve propose
octocode evolve patch <proposal-id>
octocode evolve test <run-id>
octocode evolve apply <run-id>
octocode evolve rollback <apply-id>
octocode evolve status
```

## 5. System Diagram

```text
┌──────────────────────────────────────────────────────────────┐
│                         Octocode CLI                          │
└──────────────────────────────────────────────────────────────┘
                │
                ▼
┌──────────────────────┐     ┌────────────────────────┐
│ Task Intake / Planner │────▶│ Risk & Policy Engine   │
└──────────────────────┘     └────────────────────────┘
                │                         │
                ▼                         ▼
┌──────────────────────┐     ┌────────────────────────┐
│ Repair Pipeline       │     │ Evolution Controller    │
│ locate/fix/verify     │     │ proposal/patch/test     │
└──────────────────────┘     └────────────────────────┘
                │                         │
                ▼                         ▼
┌──────────────────────┐     ┌────────────────────────┐
│ Skill/Knowledge Store │◀───▶│ Archive / Lineage Store │
└──────────────────────┘     └────────────────────────┘
                │                         │
                ▼                         ▼
┌──────────────────────┐     ┌────────────────────────┐
│ Evaluator Gates       │◀───▶│ Sandbox Runner          │
│ tests/judges/security │     │ isolated execution      │
└──────────────────────┘     └────────────────────────┘
```

## 6. Data Layout

Continue using `.octocode` as the local project runtime directory.

```text
.octocode/
  evolution/
    proposals/
      <proposal-id>.json
      <proposal-id>.md
    runs/
      <run-id>/
        proposal.json
        patch.diff
        candidate.json
        agents.json
        gates.json
        stdout.log
        stderr.log
        report.md
        worktree/
    archive/
      agents/
        <agent-id>.json
      lineage.jsonl
      scores.jsonl
    applies/
      <apply-id>/
        apply.json
        checkpoint/
    rollbacks/
      <rollback-id>.json
  knowledge/
    project.md
    risk-map.json
    failure-memory.jsonl
    world-knowledge.md
  skills/
    <skill-id>/
      SKILL.md
      examples/
      tests/
      traces.jsonl
```

## 7. Evolution Loop

### Step 1: Select an Evolution Target

Targets may come from:

- User requests.
- Recent failing tests.
- Frequently failing commands.
- Rising code complexity.
- Provider/API failures.
- Real TUI runtime issues.
- Weak benchmark capabilities.

Output: `proposal.json`.

### Step 2: Classify Risk

| Level | Scope | Automation Policy |
|---|---|---|
| Low | docs, prompts, non-core skills | may auto patch/test/apply |
| Medium | normal CLI features, provider adapters, non-security tools | requires full tests and judge review |
| High | permissions, approvals, sandboxing, command execution, secrets, core orchestration | no auto-apply by default |
| Blocked | bypass safety, delete tests, hide logs, weaken gates | reject immediately |

### Step 3: Generate Candidate Patch

Candidate sources:

- Local deterministic patch.
- Single-model code generation.
- Multi-role model collaboration: Planner / Implementer / Safety Reviewer.
- Multi-model candidates: DeepSeek, Qwen, Kimi, OpenAI-compatible, and others.

Requirements:

- Output must be a unified diff.
- Touched files must be declared.
- Expected behavior change must be declared.
- Files outside proposal targets require explicit risk escalation.

### Step 4: Sandbox Verification

Default verification ladder:

```text
format/static scan -> cargo check -> targeted tests -> full tests -> scenario replay -> judge review
```

Core self-modification should require at least:

```bash
cargo check --all-targets --all-features
cargo test --all-targets --all-features
```

If the user allows more resource usage, add:

- CLI smoke tests.
- TUI snapshot tests.
- Provider mock tests.
- Mission replay.
- Benchmark subset.

### Step 5: Multi-Judge Evaluation

Do not let the same model be the only judge of its own code.

Recommended judges:

- Rule judge: static rules, safety rules, path rules.
- Test judge: tests and scenario replay.
- Diff judge: checks whether the diff matches the proposal.
- Cost judge: duration, tokens, API cost.
- Cross-model judge: review by a different provider/model.
- Historical judge: detects repeated past failure patterns.

### Step 6: Archive the Candidate

Every candidate enters the archive, not only successful ones.

Store:

- Parent id.
- Proposal id.
- Patch hash.
- Touched files.
- Risk level.
- Gate results.
- Score delta.
- Cost.
- Failure reason.
- Judge comments.

This maps to the archive and lineage ideas from DGM/HGM.

### Step 7: Apply and Roll Back

Applying a candidate must create a checkpoint first.

```bash
octocode evolve apply <run-id>
octocode evolve rollback <apply-id>
```

Rollback reasons should be stored and used by future judges.

## 8. Token-Light Evaluation

To avoid expensive evaluation matrices, Octocode should prefer cheap signals first.

Priority order:

1. File path and risk rules.
2. Diff-level static scan.
3. Build and test results.
4. Targeted context snippets.
5. Historical failure patterns.
6. Small-model judge.
7. Large-model judge.
8. Full-context review.

A judge should usually receive:

```text
proposal summary + diff + gate output + relevant snippets + historical failures
```

It should not receive the entire repository by default.

## 9. Avoiding Same-Model Self-Confirmation

Problem: if model A writes the code and model A judges the code, it may rationalize its own mistake.

Mitigations:

- Separate generator model and reviewer model.
- Give reviewers the diff, not the generator's chain of reasoning.
- Prefer test results over natural-language approval.
- High-risk changes must pass rule gates and sandbox checks, not just model approval.
- Sample cross-provider reviews for important changes.
- Store failed cases and down-rank similar future diffs.
- Benchmark the judges themselves and track false positives/false negatives.

## 10. Octocode Constitution Draft

The self-evolution system must obey:

1. It must not bypass user permissions, approvals, sandboxing, key protection, or logging.
2. It must not delete, weaken, or skip tests to improve pass rate.
3. It must not hide failures, cost, API calls, or modification lineage.
4. It must not overwrite core code without a checkpoint.
5. It must not treat benchmark score as proof of real-world safety.
6. It must not let natural-language model judgment override deterministic gate failure.
7. Every core self-modification must be auditable, replayable, and rollback-safe.

## 11. Roadmap

### Phase A: Strong Repair Baseline

Goal: make Octocode a stable repo repair agent.

Deliverables:

- `octocode repair propose/run/report/apply`.
- File localization and patch generation pipeline.
- cargo/test/smoke gates.
- Repair run reports.

### Phase B: Experience Layer

Goal: persist project knowledge and failure memory.

Deliverables:

- `.octocode/knowledge/project.md`.
- `.octocode/knowledge/risk-map.json`.
- `.octocode/knowledge/failure-memory.jsonl`.
- `octocode knowledge update/show`.

### Phase C: Skill Layer

Goal: promote successful operations into reusable skills.

Deliverables:

- `octocode skill add/test/list/show`.
- Skill test examples.
- Skill invocation traces.
- Skill versions and applicability conditions.

### Phase D: Core Self-Evolution Upgrade

Goal: extend existing `evolve` commands with archive, lineage, multi-judge review, and tiered auto-apply.

Deliverables:

- Archive/lineage store.
- Multi-candidate patch generation.
- Cross-model judge.
- Token-light evaluation bundle.
- High-risk gate.

### Phase E: Benchmark and Lineage Search

Goal: approach DGM/HGM-style long-running evolution.

Deliverables:

- Benchmark subset runner.
- Clade score.
- Parent selection strategy.
- Regression dashboard.
- Cost-aware utility.

## 12. Minimum Viable Version

The MVP does not require full AGI. It only needs this loop:

```text
octocode evolve propose -> patch -> test -> archive -> apply/rollback
```

Add these assets around it:

- Failure memory.
- Risk map.
- Diff judge.
- Cost accounting.
- Skill promotion.

That is already a real self-improvement loop.

## 13. Acceptance Criteria

A self-evolution run is acceptable if it has:

- A clear proposal.
- A readable diff.
- A risk level.
- Gate output.
- Cost record.
- Archive record.
- Apply checkpoint.
- Rollback path.
- Replayable key steps.

Core self-modification also requires:

- No blocked patterns.
- No weakened tests or safety gates.
- Successful build.
- Relevant passing tests.
- Deterministic gate failures cannot be overridden by a model judge.

## 14. References

- Darwin Gödel Machine: Open-Ended Evolution of Self-Improving Agents
- A Self-Improving Coding Agent
- Huxley-Gödel Machine
- Gödel Agent
- Agentless
- AutoCodeRover
- SWE-agent
- SWE-bench Verified
- Agent-as-a-Judge
- Voyager
- Reflexion
- Self-Refine
- Teaching Large Language Models to Self-Debug
- Training LLM Agents for Spontaneous, Reward-Free Self-Evolution via World Knowledge Exploration
- AlphaEvolve / CodeEvolve / OpenEvolve
- yoyo-evolve
- EvoMap Evolver
- Hermes Agent Self-Evolution

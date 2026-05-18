# Octocode Repair Agent Theory

Status: local theory draft  
Scope: Octocode code repair agent, repair command group, validation loop  
Purpose: convert the Agentless, AutoCodeRover, and SWE-agent engineering path into Octocode's first implementable capability layer.

## 1. Core Position

The Repair Agent is the foundation of the Octocode self-evolution system.

Its goal is not to freely explore the whole project like a human. Its goal is to reliably complete a constrained engineering loop:

```text
problem input -> localization -> minimal patch -> validation -> report -> experience record
```

Only after this is stable should Octocode move deeper into skills, knowledge, archive, and core self-evolution.

## 2. Why Repair Agent Comes First

Reasons:

- It follows the Agentless / AutoCodeRover path, which has high engineering certainty.
- It directly helps user project repair.
- It also helps repair Octocode's own core code.
- It naturally produces validation signals, failure memory, and skill candidates.
- It is cheaper, more controllable, and easier to debug than a fully free-form agent.

Core judgment:

```text
Without reliable repair, DGM/HGM-style self-modification will amplify existing disorder.
```

## 3. Non-Goals

The first version does not do:

- Long-running free exploration.
- Unbounded whole-repository refactors.
- Automatic application of high-risk patches.
- Treating model explanation as proof of correctness.
- Expensive external benchmarks by default.
- Deleting or weakening tests to pass validation.

## 4. Basic Loop

The minimum Repair Agent loop:

```text
Input
-> Intake
-> Localization
-> Patch Planning
-> Patch Generation
-> Deterministic Validation
-> Report
-> Trace Storage
-> Failure/Skill Update
```

Commands:

```bash
octocode repair propose "..."
octocode repair run <proposal-id>
octocode repair report <run-id>
octocode repair status
```

Second phase:

```bash
octocode repair apply <run-id>
octocode repair rollback <apply-id>
```

## 5. Input Types

Repair Agent should support:

| Input | Example | Handling |
|---|---|---|
| natural language | "model switching is broken" | create repair proposal |
| test failure | cargo test output | localize relevant module |
| build failure | rustc error | localize file/symbol |
| runtime error | CLI/TUI error | analyze with trace |
| provider error | API exception | inspect provider adapter |
| regression | worked before, broken now | use failure memory |

## 6. Proposal Theory

A repair proposal is the task contract.

It must include:

```text
id
created_at
user_request
problem_statement
expected_behavior
scope_hint
risk_level
allowed_paths
blocked_paths
validation_plan
status
```

Principles:

- Define what to fix before asking a model to write code.
- A patch without proposal does not enter the repair archive.
- A clear proposal makes later judging cheaper.

## 7. Localization Theory

Localization is the key to repair because it determines token cost and patch quality.

Octocode should use layered localization:

### L0: User-hint localization

Extract modules and keywords from the user request.

### L1: Text search localization

Use `rg`, filenames, and symbol names.

### L2: Structural localization

Use language structure:

- Rust module.
- Function.
- Struct.
- Enum.
- Trait.
- CLI command branch.
- Provider adapter.

### L3: Test localization

Use failing tests, assertions, stack traces, and stderr.

### L4: Historical localization

Use failure memory, risk map, and previous repair traces.

### L5: Model-assisted localization

Use model assistance only when earlier layers are insufficient.

Principle:

```text
cheap deterministic localization before models.
```

## 8. Patch Theory

Repair patches should be minimal.

Requirements:

- Modify only files inside proposal scope.
- Avoid unrelated refactors.
- Do not delete tests to pass validation.
- Do not weaken error handling.
- Do not hide logs or failures.
- Do not touch safety/permission paths unless explicitly allowed by the proposal.

Patch output must be:

```text
unified diff + touched files + behavior change summary + validation expectation
```

## 9. Validation Theory

Repair Agent correctness comes mainly from validation, not model confidence.

Validation ladder:

| Level | Validation | Purpose |
|---|---|---|
| V1 | diff static scan | quickly reject dangerous patches |
| V2 | format/check | basic syntax and type safety |
| V3 | targeted tests | verify related behavior |
| V4 | full tests | prevent broad regressions |
| V5 | CLI smoke | verify real command entrypoints |
| V6 | scenario replay | replay real tasks |
| V7 | judge review | explanatory review |

Principles:

- V1-V3 are required for the first version.
- V4-V6 depend on risk level.
- V7 cannot override failures in V1-V6.

## 10. Diff Judge Theory

A diff judge is the most realistic first judge.

Input:

```text
proposal summary
diff
touched files
validation output
risk rules
historical failure snippets
```

Output:

```text
pass/fail/warn
reason
risk_flags
missing_tests
suggested_next_validation
```

The diff judge does not need to read the whole repository. It should answer:

- Does the patch address the proposal?
- Is the patch out of scope?
- Does it touch high-risk files?
- Does it delete or weaken tests?
- Does it hide errors?
- Is validation missing?

## 11. Risk Map Theory

Repair Agent must know which files are risky.

The risk map should store:

```text
path
risk_level
reason
required_gates
auto_apply_allowed
last_incident
```

Examples of high-risk paths:

- Policy.
- Sandbox.
- Command execution.
- Key storage.
- Provider credentials.
- Evolution controller.
- Evaluator gates.
- Approval flow.

Risk map sources:

- Static rules.
- AGENTS.md.
- Historical failures.
- User preferences.
- Manual marks.
- Automatic incidents.

## 12. Failure Memory Theory

Failure memory is what makes Repair Agent stronger over time.

Each failure should record:

```text
id
time
proposal_id
run_id
error_type
touched_files
symptom
root_cause
failed_patch_hash
validation_output
lesson
avoid_pattern
```

Usage:

- Propose stage: surface similar historical issues.
- Localization stage: prioritize historically relevant files.
- Patch stage: avoid repeated failure patterns.
- Judge stage: lower confidence for similar patches.

## 13. Skill Promotion Theory

Not every success should become a skill.

A repair run is suitable for skill promotion if:

- The problem type is repeatable.
- The steps are clear.
- Validation is stable.
- Failure boundaries are explicit.
- It does not depend on one-off context.

Examples:

- "Rust CLI command wiring fix".
- "Provider adapter smoke validation".
- "TUI snapshot regression audit".
- "Cargo feature flag repair".
- "Mission replay failure triage".

## 14. Cost Theory

Repair Agent must control token and API cost.

Cost strategy:

- Do not read the whole repository by default.
- Localize first, then read small context.
- Let judges see only diffs.
- Expand context only after failure.
- Use large models only for key patch/judge stages.
- Use small models or deterministic rules for pre-filtering.

## 15. Relationship to Self-Evolution

Repair Agent is a prerequisite for core self-evolution.

Relationship:

```text
repair user projects
-> produce failure memory / skill / risk map
-> repair Octocode itself
-> evolve calls repair pipeline to generate candidate patches
-> archive stores candidates
-> lineage selects next generation
```

So `evolve` should reuse `repair` instead of reinventing code repair.

## 16. First-Version Theory Completeness

| Module | Completeness | Implementable |
|---|---:|---|
| proposal | high | yes |
| localization | medium-high | yes, simple first version |
| patch generation | medium | yes, using existing providers first |
| validation | high | yes |
| report | high | yes |
| failure memory | high | yes |
| risk map | high | yes |
| diff judge | medium-high | yes, deterministic first |
| skill promotion | medium | skeleton only |
| apply/rollback | medium | second phase |

## 17. Recommended First Version Scope

First version should include only:

```text
repair propose
repair run
repair report
repair status
failure-memory write
risk-map read/generate
diff static judge
```

Defer:

```text
repair apply
repair rollback
multi-agent repair
benchmark integration
auto skill promotion
cross-model judge
```

Reason: stabilize the observable, replayable, verifiable loop before writing into the main workspace.

## 18. Acceptance Criteria

The first Repair Agent version is acceptable if:

- It can create a proposal.
- It can create a run directory.
- It can save a patch or candidate report.
- It can run at least one deterministic validation.
- It can produce a report.
- Failures can be written to failure memory.
- High-risk files can be flagged from the risk map.
- It does not auto-apply high-risk changes.

## 19. Final Judgment

The Repair Agent theory is complete enough to serve as Octocode's first self-evolution implementation layer.

Implementation must remain conservative:

```text
first observable, replayable, verifiable;
then automatic apply;
finally core self-evolution.
```

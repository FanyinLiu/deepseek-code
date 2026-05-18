# Octocode Self-Evolution Long-Term Roadmap

Status: local roadmap draft  
Scope: research intake, architecture iteration, implementation, dynamic validation  
Goal: make Octocode continuously improve itself based on self-evolving agent research.

## 1. Working Principle

Octocode self-evolution is not a one-time feature. It should be a long-running loop:

```text
paper/project input
-> mechanism extraction
-> Octocode module mapping
-> design and architecture update
-> small implementation slice
-> real execution validation
-> result recording
-> knowledge, risk map, and roadmap update
-> next iteration
```

Every iteration must answer four questions:

1. What implementable mechanism does the paper or project introduce?
2. Which Octocode module should absorb it?
3. What validation signal proves it works?
4. What safety, cost, or complexity risk does it introduce?

## 2. Long-Term Tracks

### Track A: Engineering Repair Agent

Goal: make Octocode a stable, measurable, rollback-safe code repair tool.

References: Agentless, AutoCodeRover, SWE-agent, SWE-bench.

Core mechanisms:

- Locate problematic files.
- Generate minimal patches.
- Validate with build, tests, and static scans.
- Produce readable reports.
- Support apply and rollback.

Octocode modules:

- `repair`
- `tools`
- `provider`
- `cli`
- `evaluation`

Priority implementation:

- `octocode repair propose`
- `octocode repair run`
- `octocode repair report`
- `octocode repair apply`
- repair run replay

### Track B: Experience Memory and Skill Library

Goal: make Octocode accumulate capability from history instead of starting from scratch every time.

References: Voyager, Reflexion, Self-Refine, Self-Debug, World Knowledge.

Core mechanisms:

- Failure reflection.
- Successful strategy promotion.
- Externalized project knowledge.
- Executable skill library.
- Automatic failure-pattern detection.

Octocode modules:

- `knowledge`
- `skills`
- `memory`
- `failure-memory`
- `project-profile`

Priority implementation:

- `.octocode/knowledge/project.md`
- `.octocode/knowledge/risk-map.json`
- `.octocode/knowledge/failure-memory.jsonl`
- `octocode knowledge update`
- `octocode skill add`
- `octocode skill test`

### Track C: Core Code Self-Evolution

Goal: let Octocode improve its own core code under sandboxed and gated conditions.

References: DGM, SICA, HGM, Gödel Agent.

Core mechanisms:

- Proposal.
- Candidate patch.
- Sandbox test.
- Archive.
- Lineage.
- Judge.
- Apply checkpoint.
- Rollback.

Octocode modules:

- `evolution`
- `archive`
- `lineage`
- `sandbox`
- `judge`
- `policy`

Priority implementation:

- Archive/lineage store.
- Multi-candidate patch generation.
- Cross-model judge.
- High-risk gate.
- Clade score.
- Cost-aware utility.

### Track D: Evaluation and Benchmarks

Goal: provide reliable, cheap, replayable selection signals for self-evolution.

References: SWE-bench Verified, AgentBench, Agent-as-a-Judge, LiveCodeBench.

Core mechanisms:

- Task sets.
- Replayable traces.
- Deterministic gates.
- Judge gates.
- Cost accounting.
- Regression dashboard.

Octocode modules:

- `benchmark`
- `eval`
- `trace`
- `report`
- `judge`

Priority implementation:

- Local benchmark subset.
- CLI smoke benchmark.
- Provider mock benchmark.
- TUI scenario benchmark.
- Process-level judge report.

## 3. Research Intake Queue

### P0: Must Absorb

| Paper/Project | Mechanism to absorb | Module | Status |
|---|---|---|---|
| Agentless | locate-fix-validate pipeline | `repair` | pending breakdown |
| AutoCodeRover | structured code search and fault localization | `repair` / `tools` | pending breakdown |
| SWE-agent | agent-computer interface and tool bundle | `tools` / `cli` | pending breakdown |
| Voyager | executable skill library | `skills` | pending breakdown |
| Reflexion | verbal reflection / episodic memory | `memory` | pending breakdown |
| DGM | archive / open-ended lineage | `evolution` / `archive` | pending breakdown |
| HGM | clade-metaproductivity | `lineage` / `selection` | pending breakdown |
| SICA | cost/time/score utility | `evaluation` | pending breakdown |
| Agent-as-a-Judge | process-level judge | `judge` | pending breakdown |

### P1: Should Track

| Paper/Project | Mechanism to absorb | Module | Status |
|---|---|---|---|
| Self-Refine | generate-feedback-refine | `repair` / `judge` | pending breakdown |
| Self-Debug | execution feedback and self-debugging | `repair` | pending breakdown |
| World Knowledge Exploration | externalized environment knowledge | `knowledge` | pending breakdown |
| AlphaEvolve | program database + evaluator | `benchmark` / `archive` | pending breakdown |
| CodeEvolve | island model / crossover | `selection` | pending breakdown |
| OpenEvolve | open AlphaEvolve-like runner | `benchmark` | pending breakdown |
| yoyo-evolve | public growth log and self-modification trace | `archive` / `report` | pending breakdown |

### P2: Observe

| Paper/Project | Question to watch | Status |
|---|---|---|
| Gödel Agent | whether runtime self-reference fits a CLI | watching |
| Self-Evolving Software Agents | BDI + automated evolution module | watching |
| EvoMap Evolver | gene/capsule/event protocolization | watching |
| Hermes Agent Self-Evolution | prompt/skill/code evolution | watching |

## 4. Implementation Path

### Phase A: Repair Baseline

Goal: build a stable code repair loop first.

Tasks:

- Add the `repair` CLI command group.
- Define repair proposal data structures.
- Define run/report/apply flow.
- Connect existing provider calls.
- Validate with diff and tests by default.
- Save repair traces.

Acceptance:

- It can generate a patch for a small Rust issue.
- It can run validation and produce a report.
- It can replay a run without relying on human explanation.
- Failed runs are written to failure memory.

### Phase B: Knowledge and Failure Memory

Goal: make Octocode remember project structure and failure modes.

Tasks:

- Add `.octocode/knowledge`.
- Generate `project.md`.
- Generate `risk-map.json`.
- Generate `failure-memory.jsonl`.
- Automatically write repair/evolve failures to failure memory.
- Let judges read historical failure patterns.

Acceptance:

- Similar failures can be detected when they recur.
- High-risk paths can be automatically tagged.
- Judges can get key context without reading the whole repository.

### Phase C: Skill Promotion

Goal: promote successful strategies into reusable skills.

Tasks:

- Add `.octocode/skills/<skill-id>`.
- Generate skill drafts from runs.
- Each skill includes description, applicability, examples, and tests.
- Repair/evolve planners can retrieve skills.
- Skill usage writes traces.

Acceptance:

- A successful run can become a skill.
- Later tasks can reference that skill.
- Skills have failure records and applicability boundaries.

### Phase D: Evolution Archive and Judges

Goal: strengthen the existing `evolve` command toward a DGM/HGM-style system.

Tasks:

- Add archive store.
- Add lineage store.
- Add patch hash and parent id.
- Add multi-candidate patches.
- Add cross-model judge.
- Add diff-only judge bundle.
- Add high-risk hard gate.

Acceptance:

- Every candidate has lineage.
- Successful and failed candidates both enter the archive.
- Judges cannot override deterministic gate failure.
- High-risk changes cannot auto-apply by default.

### Phase E: Benchmark and Selection

Goal: give long-running evolution comparable selection signals.

Tasks:

- Add local benchmark subset.
- Add smoke benchmark.
- Add score/cost/time utility.
- Add clade score.
- Add parent selection strategy.
- Add regression report.

Acceptance:

- Two candidate versions can be compared.
- Parent selection can use both cost and score.
- The system can detect candidates that improve score but increase risk.
- Evolution lineage can be tracked over time.

## 5. Paper-to-Code Process

Every absorbed paper should produce a `research intake` record:

```text
paper/project:
main mechanism:
what to copy:
what not to copy:
octocode module:
implementation task:
validation signal:
risk:
status:
```

Then proceed in this order:

1. Update roadmap.
2. Update architecture.
3. Create implementation task.
4. Implement the smallest useful slice.
5. Run dynamic validation.
6. Record trace.
7. Update knowledge / failure memory.

## 6. Recommended Next Step

Start with the minimum versions of Phase A and Phase B.

Concrete order:

1. `repair` command group.
2. repair proposal / run / report data structures.
3. repair run trace.
4. failure-memory write path.
5. risk-map generation.
6. diff judge.
7. promote successful repair into a skill.

Reasoning:

- This follows the Agentless / AutoCodeRover path, which has the strongest engineering certainty.
- It serves both normal project repair and Octocode self-evolution.
- Without a stable repair baseline, DGM/HGM-style self-modification will amplify existing disorder.

## 7. Risk Controls

Continuously monitor:

- Token cost growth.
- Judge self-confirmation.
- Benchmark hacking.
- Weakened tests.
- High-risk files being auto-applied.
- Archive storing only successes and losing failure experience.
- Skills overgeneralizing and being reused incorrectly.
- Provider differences causing non-reproducible behavior.

## 8. Work Record Rules

After every implementation iteration, update at least:

- Current roadmap status.
- Related architecture sections.
- New command documentation.
- Validation result.
- Risk changes.
- Next iteration plan.

For core self-evolution changes, also record:

- Touched files.
- Risk level.
- Gate result.
- Rollback path.
- Archive id.
- Failure memory delta.

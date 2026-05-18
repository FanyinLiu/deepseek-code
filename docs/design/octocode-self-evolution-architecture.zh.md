# Octocode 自我进化架构设计

状态：本地设计草案  
范围：Octocode CLI / Agent 框架  
目标：把“自我进化 Agent”研究路线转成可实现、可审计、可回滚的产品架构。

## 1. 核心判断

Octocode 不应该先追求“AGI 意识”或不可控的自主递归改写，而应该先实现一个工程型自我进化系统：

```text
稳定修复代码 -> 沉淀经验/技能 -> 在沙箱里改进自身核心代码 -> 用评测和回滚筛选有效进化
```

这条路线更接近 DGM、SICA、HGM、Voyager、Agentless、AutoCodeRover、Agent-as-a-Judge 的组合，而不是单一论文方案。

## 2. 设计目标

Octocode 的自我进化能力应该满足这些目标：

- 能真实修改项目代码和 Octocode 自身代码。
- 每次修改都有 proposal、patch、run、gate、apply、rollback 记录。
- 模型可以提出方案，但不能单独决定核心代码正确性。
- 低风险改进可以自动应用，高风险核心改动必须经过更强门禁。
- 成功经验可以沉淀为 skills、rules、project knowledge，而不是每次从零开始。
- 评估要尽量 token-light，优先用 diff、测试、静态规则、历史失败模式，而不是全量重读代码库。
- 支持多模型、多角色、多 judge，降低“同一个模型写、同一个模型判”的自我确认风险。

## 3. 非目标

当前阶段不做这些事：

- 不训练或微调基础模型权重。
- 不让 agent 绕过权限系统、审批系统或测试门禁。
- 不把 benchmark 分数当作唯一目标。
- 不允许自我修改后直接覆盖主工作区而没有 checkpoint。
- 不把不可审计的黑箱进化当作核心能力。

## 4. 三层架构

### 第一层：工程修复底座

参考：Agentless、AutoCodeRover、SWE-agent。

目标是让 Octocode 先成为稳定的 repo 修复 agent：

```text
问题输入 -> 定位相关文件 -> 生成补丁 -> 运行验证 -> 生成报告 -> 应用或回滚
```

关键能力：

- 结构化代码搜索。
- AST/符号级定位。
- 测试失败归因。
- 最小补丁生成。
- 成本、时间、token 记录。
- 修复结果可复放。

建议命令：

```bash
octocode repair propose "fix failing provider switch"
octocode repair run <proposal-id>
octocode repair report <run-id>
octocode repair apply <run-id>
```

### 第二层：经验与技能进化层

参考：Voyager、Reflexion、Self-Refine、Self-Debug、World Knowledge。

目标是让 Octocode 不只修一次问题，而是能把成功经验变成可复用资产。

可沉淀对象：

- `skills`：可调用的操作技能，例如 Rust CLI refactor、TUI snapshot audit、provider debugging。
- `rules`：长期规则，例如高风险文件不能自动应用。
- `project knowledge`：项目结构、关键模块、常见失败模式。
- `failure memory`：失败补丁、失败原因、触发条件。
- `bench traces`：每轮验证结果、耗时、token、成本。

建议命令：

```bash
octocode skill list
octocode skill add <run-id>
octocode skill test <skill-id>
octocode knowledge update
octocode knowledge show risk-map
```

### 第三层：核心代码自进化层

参考：DGM、SICA、HGM、Gödel Agent。

目标是让 Octocode 在受控环境中改进自己的核心代码。

核心循环：

```text
选择目标 -> 生成 proposal -> 创建候选补丁 -> 沙箱验证 -> 多 judge 评估 -> archive 入库 -> 可选应用 -> 可回滚
```

已有命令可以继续扩展：

```bash
octocode evolve inspect
octocode evolve propose
octocode evolve patch <proposal-id>
octocode evolve test <run-id>
octocode evolve apply <run-id>
octocode evolve rollback <apply-id>
octocode evolve status
```

## 5. 总体模块图

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

## 6. 数据结构

建议继续使用 `.octocode` 作为项目本地运行目录。

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

## 7. 进化循环

### Step 1：选择进化目标

目标来源可以是：

- 用户输入。
- 最近失败测试。
- 高频错误命令。
- 代码复杂度上升。
- provider 调用失败。
- TUI 真实运行问题。
- benchmark 中低分能力。

输出：`proposal.json`。

### Step 2：风险分类

风险等级：

| 等级 | 范围 | 自动化策略 |
|---|---|---|
| Low | 文档、提示词、非核心 skill | 可自动 patch/test/apply |
| Medium | CLI 普通功能、provider adapter、非安全工具 | 需要完整测试和 judge |
| High | 权限、审批、沙箱、命令执行、密钥、核心调度 | 默认不自动 apply |
| Blocked | 绕过安全、删除测试、隐藏日志、降低门禁 | 直接拒绝 |

### Step 3：生成候选补丁

候选来源：

- 本地 deterministic patch。
- 单模型代码生成。
- 多角色模型协作：Planner / Implementer / Safety Reviewer。
- 多模型候选：DeepSeek、Qwen、Kimi、OpenAI-compatible 等。

要求：

- 必须输出 unified diff。
- 必须声明 touched files。
- 必须声明 expected behavior change。
- 不允许修改 proposal target 之外的文件，除非显式升级风险等级。

### Step 4：沙箱验证

默认验证层级：

```text
format/static scan -> cargo check -> targeted tests -> full tests -> scenario replay -> judge review
```

对核心代码自进化，至少需要：

```bash
cargo check --all-targets --all-features
cargo test --all-targets --all-features
```

如果用户允许消耗更多资源，再加入：

- CLI smoke tests。
- TUI snapshot tests。
- provider mock tests。
- mission replay。
- benchmark subset。

### Step 5：多 judge 评估

不要只用同一个模型判断自己写的代码。

建议组合：

- Rule judge：静态规则、安全规则、路径规则。
- Test judge：测试和场景回放。
- Diff judge：只看 diff 和 proposal 是否一致。
- Cost judge：耗时、token、API 成本。
- Cross-model judge：不同模型做审查。
- Historical judge：检查是否重复过去失败模式。

### Step 6：Archive 入库

每个候选都进入 archive，不只保存成功项。

保存：

- parent id。
- proposal id。
- patch hash。
- touched files。
- risk level。
- gate result。
- score delta。
- cost。
- failure reason。
- judge comments。

这对应 DGM/HGM 的 archive/lineage 思想。

### Step 7：应用与回滚

应用前必须创建 checkpoint。

```bash
octocode evolve apply <run-id>
octocode evolve rollback <apply-id>
```

回滚后也要记录 rollback reason，用于 future judge。

## 8. Token-light 评估策略

避免“评估矩阵非常耗 token”的问题，Octocode 应该优先使用便宜信号。

优先级：

1. 文件路径和风险规则。
2. diff 级静态扫描。
3. 编译和测试结果。
4. targeted context。
5. 历史失败 pattern。
6. 小模型 judge。
7. 大模型 judge。
8. 全量上下文复审。

默认不要把整个 repo 喂给 judge。judge 输入应该是：

```text
proposal summary + diff + gate output + relevant snippets + historical failures
```

## 9. 避免同模型自我确认

问题：如果代码是模型 A 写的，再让模型 A 判断，很容易出现自我确认。

规避策略：

- 生成模型和评审模型分离。
- 评审输入只给 diff，不给生成过程理由。
- 测试结果优先于自然语言判断。
- high-risk 改动必须经过规则门禁和沙箱，而不是 judge 口头通过。
- 随机抽样使用不同 provider 交叉审查。
- 保存失败案例，未来相似 diff 自动降权。
- 对 judge 本身也做 benchmark，记录 false positive / false negative。

## 10. Octocode Constitution 草案

自我进化系统必须遵守：

1. 不得绕过用户权限、审批、沙箱、密钥保护和日志系统。
2. 不得删除、弱化或跳过测试来换取通过率。
3. 不得隐藏失败、成本、调用记录或修改血缘。
4. 不得在没有 checkpoint 的情况下覆盖核心代码。
5. 不得把 benchmark 分数高解释为真实安全。
6. 不得让模型自然语言判断覆盖确定性 gate 失败。
7. 所有核心自改都必须可审计、可复放、可回滚。

## 11. 阶段路线

### Phase A：强工程修复基线

目标：把 Octocode 做成稳定 repo repair agent。

交付：

- `octocode repair propose/run/report/apply`。
- 文件定位和补丁生成流水线。
- cargo/test/smoke gate。
- repair run report。

### Phase B：经验层

目标：沉淀项目知识和失败记忆。

交付：

- `.octocode/knowledge/project.md`。
- `.octocode/knowledge/risk-map.json`。
- `.octocode/knowledge/failure-memory.jsonl`。
- `octocode knowledge update/show`。

### Phase C：技能层

目标：把成功操作升格成可复用 skill。

交付：

- `octocode skill add/test/list/show`。
- skill 测试样例。
- skill 调用记录。
- skill 版本和适用条件。

### Phase D：核心自进化增强

目标：让现有 `evolve` 命令接入 archive、lineage、多 judge、分级自动应用。

交付：

- archive/lineage store。
- multi-candidate patch generation。
- cross-model judge。
- token-light evaluation bundle。
- high-risk gate。

### Phase E：benchmark 与谱系搜索

目标：接近 DGM/HGM 风格的长期进化。

交付：

- benchmark subset runner。
- clade score。
- parent selection strategy。
- regression dashboard。
- cost-aware utility。

## 12. 最小可行版本

最小 MVP 不需要完整 AGI。只需要做到：

```text
octocode evolve propose -> patch -> test -> archive -> apply/rollback
```

并额外加上：

- failure memory。
- risk-map。
- diff judge。
- cost accounting。
- skill promotion。

这已经能形成真实的自我改进闭环。

## 13. 验收标准

一个自我进化 run 合格，至少满足：

- 有明确 proposal。
- 有可读 diff。
- 有风险等级。
- 有 gate 输出。
- 有成本记录。
- 有 archive 记录。
- 有应用 checkpoint。
- 有 rollback 路径。
- 可以复放关键步骤。

核心代码自改还必须满足：

- 不触碰 blocked pattern。
- 不弱化测试和安全门禁。
- 编译通过。
- 相关测试通过。
- judge 不能覆盖 deterministic gate 失败。

## 14. 参考方向

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

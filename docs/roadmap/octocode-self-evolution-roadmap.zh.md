# Octocode 自我进化长期路线图

状态：本地路线草案  
范围：论文吸收、架构迭代、代码实现、动态验证  
目标：让 Octocode 持续根据自我进化 Agent 研究进展改进自身能力。

## 1. 工作原则

Octocode 的自我进化不是一次性功能，而是一条长期循环：

```text
论文/项目输入
-> 提取机制
-> 映射到 Octocode 模块
-> 更新设计与架构
-> 小步实现
-> 真实运行验证
-> 记录结果
-> 更新知识库、风险图和路线图
-> 进入下一轮
```

每一轮都必须回答四个问题：

1. 这篇论文或项目提出了什么可实现机制？
2. 它应该进入 Octocode 的哪个模块？
3. 它需要什么验证信号证明有效？
4. 它会引入什么安全、成本或复杂度风险？

## 2. 长期方向

### 方向 A：工程修复 Agent

目标：让 Octocode 先成为稳定、可测、可回滚的代码修复工具。

参考：Agentless、AutoCodeRover、SWE-agent、SWE-bench。

核心机制：

- 定位问题文件。
- 生成最小补丁。
- 使用编译、测试、静态扫描验证。
- 生成可读报告。
- 支持应用和回滚。

Octocode 模块：

- `repair`
- `tools`
- `provider`
- `cli`
- `evaluation`

优先实现：

- `octocode repair propose`
- `octocode repair run`
- `octocode repair report`
- `octocode repair apply`
- repair run replay

### 方向 B：经验记忆与技能库

目标：让 Octocode 从历史任务中积累能力，而不是每轮从零开始。

参考：Voyager、Reflexion、Self-Refine、Self-Debug、World Knowledge。

核心机制：

- 失败反思。
- 成功策略沉淀。
- 项目知识外显。
- 可执行 skill 库。
- 失败模式自动识别。

Octocode 模块：

- `knowledge`
- `skills`
- `memory`
- `failure-memory`
- `project-profile`

优先实现：

- `.octocode/knowledge/project.md`
- `.octocode/knowledge/risk-map.json`
- `.octocode/knowledge/failure-memory.jsonl`
- `octocode knowledge update`
- `octocode skill add`
- `octocode skill test`

### 方向 C：核心代码自进化

目标：让 Octocode 在沙箱和门禁内改进自己的核心代码。

参考：DGM、SICA、HGM、Gödel Agent。

核心机制：

- proposal。
- candidate patch。
- sandbox test。
- archive。
- lineage。
- judge。
- apply checkpoint。
- rollback。

Octocode 模块：

- `evolution`
- `archive`
- `lineage`
- `sandbox`
- `judge`
- `policy`

优先实现：

- archive/lineage store。
- multi-candidate patch。
- cross-model judge。
- high-risk gate。
- clade score。
- cost-aware utility。

### 方向 D：评测与基准

目标：给自我进化提供可靠、便宜、可复放的选择信号。

参考：SWE-bench Verified、AgentBench、Agent-as-a-Judge、LiveCodeBench。

核心机制：

- 任务集。
- 可复放 trace。
- deterministic gate。
- judge gate。
- cost accounting。
- regression dashboard。

Octocode 模块：

- `benchmark`
- `eval`
- `trace`
- `report`
- `judge`

优先实现：

- local benchmark subset。
- CLI smoke benchmark。
- provider mock benchmark。
- TUI scenario benchmark。
- process-level judge report。

## 3. 论文吸收队列

### P0：必须吸收

| 论文/项目 | 要吸收的机制 | 对应模块 | 状态 |
|---|---|---|---|
| Agentless | locate-fix-validate 简单强流水线 | `repair` | 待拆解 |
| AutoCodeRover | 结构化代码搜索和 fault localization | `repair` / `tools` | 待拆解 |
| SWE-agent | agent-computer interface 和 tool bundle | `tools` / `cli` | 待拆解 |
| Voyager | 可执行 skill library | `skills` | 待拆解 |
| Reflexion | verbal reflection / episodic memory | `memory` | 待拆解 |
| DGM | archive / open-ended lineage | `evolution` / `archive` | 待拆解 |
| HGM | clade-metaproductivity | `lineage` / `selection` | 待拆解 |
| SICA | cost/time/score utility | `evaluation` | 待拆解 |
| Agent-as-a-Judge | 过程级 judge | `judge` | 待拆解 |

### P1：应该跟进

| 论文/项目 | 要吸收的机制 | 对应模块 | 状态 |
|---|---|---|---|
| Self-Refine | generate-feedback-refine | `repair` / `judge` | 待拆解 |
| Self-Debug | 执行反馈和自调试 | `repair` | 待拆解 |
| World Knowledge Exploration | 环境知识外显 | `knowledge` | 待拆解 |
| AlphaEvolve | program database + evaluator | `benchmark` / `archive` | 待拆解 |
| CodeEvolve | island model / crossover | `selection` | 待拆解 |
| OpenEvolve | 开源 AlphaEvolve-like runner | `benchmark` | 待拆解 |
| yoyo-evolve | 公开成长日志和自改 trace | `archive` / `report` | 待拆解 |

### P2：观察

| 论文/项目 | 要观察的问题 | 状态 |
|---|---|---|
| Gödel Agent | runtime self-reference 是否适合 CLI | 观察 |
| Self-Evolving Software Agents | BDI + automated evolution module | 观察 |
| EvoMap Evolver | gene/capsule/event 协议化 | 观察 |
| Hermes Agent Self-Evolution | prompt/skill/code evolution | 观察 |

## 4. 实现路线

### Phase A：Repair Baseline

目标：先做稳定代码修复闭环。

任务：

- 增加 `repair` CLI 命令组。
- 建立 repair proposal 数据结构。
- 建立 run/report/apply 流程。
- 接入现有 provider 调用。
- 默认使用 diff + tests 验证。
- 保存 repair traces。

验收：

- 可以对一个小型 Rust 问题生成补丁。
- 可以运行验证并生成报告。
- 可以不依赖人工解释复放 run。
- 失败 run 会进入 failure memory。

### Phase B：Knowledge and Failure Memory

目标：让 Octocode 记住项目结构和失败模式。

任务：

- 增加 `.octocode/knowledge`。
- 生成 `project.md`。
- 生成 `risk-map.json`。
- 生成 `failure-memory.jsonl`。
- repair/evolve 失败时自动写入 failure memory。
- judge 读取历史失败 pattern。

验收：

- 同类失败再次出现时可以被识别。
- 高风险路径可以被自动标记。
- judge 不需要全量读取 repo 就能获得关键上下文。

### Phase C：Skill Promotion

目标：把成功策略升格成可复用技能。

任务：

- 增加 `.octocode/skills/<skill-id>`。
- 支持从 run 生成 skill 草案。
- 每个 skill 包含说明、适用条件、示例、测试。
- repair/evolve planner 可以检索 skill。
- skill 使用结果写入 traces。

验收：

- 成功 run 可以转成 skill。
- 后续任务可以引用 skill。
- skill 有失败记录和适用边界。

### Phase D：Evolution Archive and Judges

目标：强化现有 `evolve`，让它接近 DGM/HGM 风格。

任务：

- 增加 archive store。
- 增加 lineage store。
- 增加 patch hash 和 parent id。
- 增加多候选补丁。
- 增加 cross-model judge。
- 增加 diff-only judge bundle。
- 增加 high-risk hard gate。

验收：

- 每个候选都有 lineage。
- 成功和失败候选都入 archive。
- judge 不能覆盖 deterministic gate 失败。
- 高风险改动默认不能自动 apply。

### Phase E：Benchmark and Selection

目标：让长期进化有可比较的选择信号。

任务：

- 增加本地 benchmark subset。
- 增加 smoke benchmark。
- 增加 score/cost/time utility。
- 增加 clade score。
- 增加 parent selection strategy。
- 增加 regression report。

验收：

- 可以比较两个候选版本。
- 可以基于成本和分数选择 parent。
- 可以发现性能提升但风险变大的候选。
- 可以长期追踪进化谱系。

## 5. 每轮论文到代码的流程

每吸收一篇论文，必须产出一个 `research intake` 记录：

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

然后按这个顺序推进：

1. 更新 roadmap。
2. 更新 architecture。
3. 建 implementation task。
4. 实现最小切片。
5. 动态测试。
6. 记录 trace。
7. 更新 knowledge / failure memory。

## 6. 当前推荐的下一步

推荐先做 Phase A 和 Phase B 的最小版本。

具体顺序：

1. `repair` 命令组。
2. repair proposal / run / report 数据结构。
3. repair run trace。
4. failure-memory 写入。
5. risk-map 生成。
6. diff judge。
7. 再把成功 repair 提升为 skill。

理由：

- 这是 Agentless / AutoCodeRover 路线，工程确定性最高。
- 它可以服务普通项目修复，也可以服务 Octocode 自身进化。
- 没有稳定 repair 底座，DGM/HGM 式自改会放大混乱。

## 7. 风险控制

必须持续监控：

- token 成本膨胀。
- judge 自我确认。
- benchmark hacking。
- 测试被弱化。
- 高风险文件被自动应用。
- archive 只保存成功项导致失去失败经验。
- skill 过度泛化导致错误复用。
- provider 差异导致行为不可复现。

## 8. 工作记录规范

每轮实现后，至少更新：

- roadmap 当前状态。
- architecture 相关章节。
- 新增命令说明。
- 验证结果。
- 风险变化。
- 下一轮计划。

如果是核心自进化相关改动，还必须记录：

- touched files。
- risk level。
- gate result。
- rollback path。
- archive id。
- failure memory delta。

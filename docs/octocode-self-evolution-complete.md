# Octocode Self-Evolution Complete Dossier / Octocode 自我进化完整档案

状态：本地整合版
生成时间：2026-05-17 03:14:16 PDT
范围：research map, theory, repair, knowledge/skill, archive/judge/selection, architecture, roadmap

## 使用说明 / How to Use

这是当前 Octocode 自我进化方向的单一入口文档。中文部分优先用于产品和实现决策，英文部分用于交给其他 agent 或外部研究工具。

---

# Part I. 中文总览

## 1. 当前结论

Octocode 应先成为可验证、可回滚、可积累经验的工程型自我进化 CLI，而不是直接追求无约束 AGI。推荐顺序是 repair + knowledge + failure memory，再做 skill + judge + archive，最后再做 HGM/DGM 风格核心自我进化搜索。

## 2. 第一阶段实施边界

第一阶段只应实现：repair propose/run/report/status、risk-map、failure-memory、diff/static judge、基础 archive record。暂缓：高风险自动 apply、policy/sandbox 自改、长时间无监督谱系搜索。

---

# Part II. 中文原始文档整合


---

<!-- Source: docs/research/octocode-self-evolution-research-map.zh.md -->
# Octocode 自我进化研究地图

> 日期：2026-05-17  
> 目标：为 Octocode 的“核心代码自我改进能力”建立论文、项目和工程落地路线。  
> 结论：Octocode 不应追求无约束 AGI，而应构建“受约束的自我改进工程系统”：模型提出候选，程序化 gate 判断证据，低风险自动应用，高风险隔离。

## 1. 核心问题

Octocode 想要的能力不是普通 coding agent，而是 recursive self-improvement 的工程化版本：

```text
观察自身运行结果
-> 发现失败和能力缺口
-> 生成改进 proposal
-> 生成候选 patch
-> 运行测试和安全 gate
-> 自动应用或隔离
-> 失败回滚
-> 把经验沉淀成规则、测试、技能或代码
```

关键风险是：不能让同一个模型“生成代码、定义目标、判断正确、批准上线”。这会产生 self-confirmation bias、reward hacking、删测试、绕过安全边界等问题。

Octocode 的基本原则应是：

```text
模型可以提出进化，但不能单独批准进化。
```

## 2. 论文主线

| 方向 | 代表工作 | 核心思想 | 对 Octocode 的启发 |
|---|---|---|---|
| 反思式改进 | Reflexion | 不更新模型权重，而是把失败反馈写成语言记忆，指导下一次尝试。 | 把失败 run、rollback、用户打断写成 evolution memory，影响后续 proposal。 |
| 自反馈迭代 | Self-Refine | 同一模型生成、反馈、改写，形成 test-time refinement。 | 可用于低风险文档、prompt、agent spec 改进；不能作为核心代码最终裁判。 |
| 技能库成长 | Voyager | 自动 curriculum、技能库、执行反馈、自验证，持续获得新能力。 | Octocode 应把成功流程沉淀为 skills、agents、test recipes，而不只改核心代码。 |
| 软件工程 agent | SWE-agent | 面向 GitHub issue 的 coding agent，强调工具接口、环境和可执行验证。 | `evolve` 应以真实 repo、真实测试、真实 CLI smoke 为判断核心。 |
| 少 agent / 无复杂框架 | Agentless | 通过检索、定位、修复、验证的简化流程解决 SWE-bench。 | 自我进化不一定需要巨大 swarm；高质量定位和验证比角色数量更重要。 |
| 自编辑 agent | A Self-Improving Coding Agent | agent 修改自身代码，并用 benchmark 验证性能提升。 | Octocode 可以对自身代码做 proposal -> patch -> benchmark，但必须保留 rollback。 |
| 开放式自改进 | Darwin Gödel Machine | 维护 agent archive，采样旧 agent 生成新 agent，用 benchmark 验证。 | Octocode 可维护 evolution archive，而不是只保留单线最新版本。 |
| 演化式代码搜索 | AlphaEvolve | LLM 生成程序，自动 evaluator 验证，程序数据库保留优秀变体。 | Octocode 应把 evaluator/gate 放在模型之上，用自动评分替代模型自信。 |
| 开源 AlphaEvolve 类实现 | OpenEvolve | 面向代码优化的开放实现，强调 prompt、约束和 program database。 | 可借鉴 program database、prompt stochasticity、domain-specific evaluator。 |
| 经验生命周期 | EvolveR / CodeEvolve 等 | 运行经验驱动选择目标、优化代码、沉淀改进。 | Octocode 需要从 logs 和失败事件中选择下一轮进化目标。 |

## 3. 类项目地图

### 3.1 yoyo / yoyo-evolve

- 项目：`yologdev/yoyo-evolve`
- 定位：从约 200 行 CLI 起步的自我演化 coding agent 实验。
- 机制：每轮选择一个改进点，修改自身源码，运行构建/测试，写 journal，公开成长过程。
- 重要启发：
  - 自我进化需要“叙事日志”和“成长轨迹”，否则无法判断长期趋势。
  - 每次只改一个重点，比一次大规模自改更稳定。
  - 社区 issue 可作为外部现实反馈，避免 agent 只优化自己的幻觉目标。
- 对 Octocode 的取舍：
  - 应借鉴 journal、CI、one-change-per-session。
  - 不应照搬“成长实验”形态；Octocode 是工具产品，需要更强 gate、rollback 和风险分级。

### 3.2 EvoMap / Evolver

- 定位：GEP-powered self-evolving engine，强调 Genes、Capsules、Events 和 auditable evolution。
- 重要启发：
  - 进化单元应可审计、可版本化、可回放。
  - evolution 不一定直接改代码，也可以先产出 prompt/gene/capsule 作为中间产物。
- 对 Octocode 的取舍：
  - 可借鉴“事件流 + 进化基因”的表示。
  - Octocode 仍应把源码 patch 作为最终可验证产物。

### 3.3 Hermes Agent Self-Evolution

- 定位：围绕技能/skill 文件的自我演化，而不是直接改核心 runtime。
- 重要启发：
  - 技能级自我改进风险比核心代码低，适合自动化。
  - 可以先优化 agent skills、workflow、prompt，再升级到核心代码。
- 对 Octocode 的取舍：
  - 应增加“skill/agent evolution lane”。
  - 核心代码 lane 必须更严格。

### 3.4 OpenEvolve / AlphaEvolve 类系统

- 定位：用 LLM 生成程序变体，用 evaluator 选择更好的程序。
- 重要启发：
  - 评价器比生成器更重要。
  - program database 可以避免只沿一条错误路线前进。
  - 多模型 ensemble 可提高探索广度和深度。
- 对 Octocode 的取舍：
  - 不应每次全仓库喂模型。
  - 应只喂 proposal、diff、target excerpt、test result。
  - 应建立 run archive 和候选 patch archive。

## 4. Octocode 架构建议

### 4.1 五层自我改进

1. 配置级：自动调整 provider、model、context、routing、test matrix。
2. 技能级：自动新增/修改 agents、skills、workflows、prompt recipes。
3. 外围代码级：docs、tests、examples、provider adapters、UI polish。
4. 核心代码级：orchestrator、tools、policy、provider、TUI runtime。
5. 目标函数级：修改“什么是更好”的判断标准。

默认自动化边界：

```text
可自动：配置级、技能级、低风险外围代码
可半自动：普通功能代码
默认隔离：核心代码
禁止自动：policy / defense / keyring / command dispatch / approval bypass
```

### 4.2 进化闭环

```text
Observe
  收集失败任务、rollback、test failure、token waste、用户打断、UI 卡点

Diagnose
  分类为模型问题、上下文问题、工具问题、测试不足、UI 问题、provider 问题

Propose
  生成 evolution proposal，声明目标、范围、风险、预期指标

Generate
  本地规则 + 模型 planner/implementer/safety reviewer 生成候选 patch

Verify
  路径规则、编译、测试矩阵、安全扫描、CLI smoke、异构模型反证

Apply
  低风险自动 apply；核心代码进入 quarantine；失败自动 rollback

Consolidate
  成功经验写入 tests / docs / rules / skills / routing heuristics
```

### 4.3 自动判断不要靠 token

测试矩阵不应该是“把全部代码发给模型判断”。大部分 gate 是零 token：

- `cargo check`
- `cargo test`
- clippy / fmt
- patch path scan
- protected path scan
- secret scan
- approval bypass scan
- CLI smoke
- TUI snapshot

模型只看小上下文：

```text
proposal + diff + touched file excerpt + test result + constitution
```

### 4.4 规避同模型自证

- 生成模型不能做最终裁判。
- judge 不看 generator 的解释，只看 spec、diff、测试结果。
- 反证式审查优先：问“这个 patch 可能怎么错”，不要问“它对吗”。
- 尽量异构模型：Qwen 生成，Kimi/DeepSeek 审查，或反过来。
- 最终准入以可执行证据为主。

## 5. Octocode Constitution 草案

1. 模型不是最终裁判。
2. 任何核心进化必须可测试、可回滚、可审计。
3. 不允许绕过 approval、sandbox、keyring、protected paths。
4. 不允许通过删除测试、降低安全、隐藏错误来提高指标。
5. 低风险改动可自动应用，高风险改动必须隔离。
6. 进化目标必须服务于多模型、多智能体、多能力、自我验证。
7. 所有自动改进都必须留下 proposal、patch、gate、apply、rollback 记录。
8. 修改目标函数或 constitution 本身必须进入最高风险隔离区。

## 6. 对当前 Octocode 的落地路线

### Phase A：研究和文档

- 建立论文索引和类项目索引。
- 把每篇论文抽象成 Octocode 可实现能力。
- 建立 `Octocode Constitution`。

### Phase B：自动测试矩阵

- 根据 touched files 自动选择测试：
  - `src/provider/**` -> provider tests + API mock
  - `src/evolution/**` -> evolution CLI tests
  - `src/policy/**` / `src/defense/**` -> security tests
  - `src/tui/**` -> TUI snapshot / preview smoke
- 增加 `evolve test --matrix auto`。

### Phase C：异构审查

- 增加 `evolve judge`。
- 支持 `--reviewer-provider qwen/kimi/deepseek/openrouter`。
- judge 只做反证、测试建议和 constitution violation 检查。

### Phase D：Autopilot 分级

- `tier low`：自动应用 docs/tests/examples。
- `tier medium`：自动应用普通功能代码，失败回滚。
- `tier core-safe`：允许核心代码但禁止安全边界文件。
- `tier blocked`：policy/defense/keyring/dispatch 永不自动应用。

### Phase E：进化记忆

- 保存成功/失败 run 的特征。
- 把失败模式写入反例库。
- 把成功模式写入 skills、tests、routing rules。

## 7. 推荐阅读顺序

1. Reflexion：理解语言反馈和 episodic memory。
2. Self-Refine：理解自反馈迭代的能力和局限。
3. Voyager：理解 skill library 和 lifelong learning。
4. SWE-agent / Agentless：理解真实软件工程任务和工具接口。
5. A Self-Improving Coding Agent：理解 agent 自编辑自身代码。
6. Darwin Gödel Machine：理解 archive-based open-ended self-improvement。
7. AlphaEvolve / OpenEvolve：理解 evaluator、program database 和演化式搜索。
8. yoyo-evolve：理解公开成长、自我 journal、CI 约束的工程实验。
9. EvoMap / Hermes：理解 skill-level evolution 和 auditable evolution。

## 8. 参考资料

- Reflexion: https://arxiv.org/abs/2303.11366
- Self-Refine: https://arxiv.org/abs/2303.17651
- Voyager: https://arxiv.org/abs/2305.16291
- SWE-agent: https://github.com/princeton-nlp/SWE-agent
- Agentless: https://arxiv.org/search/?query=Agentless+software+engineering+agent&searchtype=all
- A Self-Improving Coding Agent: https://arxiv.org/abs/2504.15228
- Darwin Gödel Machine: https://arxiv.org/abs/2505.22954
- AlphaEvolve: https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/
- OpenEvolve: https://github.com/algorithmicsuperintelligence/openevolve
- EvoMap Evolver: https://github.com/EvoMap/evolver
- yoyo-evolve: https://github.com/yologdev/yoyo-evolve
- yoyo public journal: https://yologdev.github.io/yoyo-evolve/

---

<!-- Source: docs/theory/octocode-self-evolution-theory.zh.md -->
# Octocode 自我进化理论框架

状态：本地理论草案  
范围：Octocode CLI、Agent、Repair、Knowledge、Skill、Evolution 体系  
目的：在改代码前先定义清楚 Octocode 的自我进化理论边界、正确性来源、评估机制和安全约束。

## 1. 一句话定义

Octocode 的自我进化不是“模型自己变成 AGI”，而是：

```text
一个可执行、可评测、可审计、可回滚的工程系统，持续改进自己的工具、技能、记忆、策略和部分核心代码。
```

它的核心不是意识，而是闭环：

```text
观察 -> 假设 -> 修改 -> 验证 -> 记录 -> 选择 -> 沉淀 -> 下一轮
```

## 2. 理论边界

### Octocode 可以做到的

- 修改用户项目代码。
- 在沙箱中修改 Octocode 自己的代码。
- 从失败中记录 failure memory。
- 从成功中生成 skill。
- 从项目中生成 project knowledge。
- 用测试、静态规则、diff judge、多模型 judge 评估补丁。
- 用 archive 和 lineage 追踪不同版本的演化。
- 用 rollback 逆转失败进化。
- 用成本、时间、风险、质量组成多目标 utility。

### Octocode 暂时不应该宣称做到的

- 不宣称拥有意识。
- 不宣称能证明自己永远正确。
- 不宣称能无约束地安全递归自改。
- 不宣称 benchmark 分数等于真实世界安全。
- 不宣称一个模型能可靠评判自己写的所有核心代码。

## 3. 基本公理

### 公理 1：模型不是正确性来源

模型可以提出候选方案，但正确性必须来自外部信号。

外部信号包括：

- 编译结果。
- 测试结果。
- 静态扫描。
- 安全规则。
- 运行 trace。
- 用户约束。
- 多模型审查。
- 历史失败记忆。

### 公理 2：自我进化首先改 scaffold，不首先改模型权重

短中期最现实的进化对象是：

- prompts。
- tool descriptions。
- command workflows。
- code repair pipeline。
- skill library。
- project knowledge。
- evaluator gates。
- selection strategy。
- CLI core code。

不是基础模型权重。

### 公理 3：没有评测，就没有进化

不能被验证的修改，只能算生成，不能算进化。

一次进化必须至少产生：

- proposal。
- patch。
- verification result。
- score or gate result。
- trace。
- archive record。

### 公理 4：失败也是资产

失败补丁、失败测试、错误 judge、错误路线都必须保存。

因为长期进化依赖：

- 避免重复错误。
- 识别风险模式。
- 训练/校准 judge。
- 判断 skill 的适用边界。
- 改进 parent selection。

### 公理 5：自改必须可回滚

任何核心自改如果不能 checkpoint 和 rollback，就不应该 apply。

### 公理 6：选择函数必须多目标

不能只优化 benchmark 分数。

Octocode 的 utility 至少包括：

```text
quality + safety + cost + latency + maintainability + reversibility + user value
```

## 4. 能力分层

Octocode 的自我进化应分成五层，而不是混在一起。

### L1：反思层

输入：一次失败或成功任务。  
输出：文字反思、错误原因、下一次建议。

参考：Reflexion、Self-Refine、Self-Debug。

作用：提升局部修复质量。

### L2：知识层

输入：项目结构、历史任务、失败模式。  
输出：project knowledge、risk-map、world knowledge。

参考：World Knowledge Exploration。

作用：让 agent 理解当前环境，而不是每次从零读 repo。

### L3：技能层

输入：重复成功的操作流程。  
输出：可复用 skill，包括说明、适用场景、示例、测试。

参考：Voyager。

作用：把经验变成可调用能力。

### L4：工程修复层

输入：真实 issue、失败测试、用户需求。  
输出：补丁、验证报告、repair trace。

参考：Agentless、AutoCodeRover、SWE-agent。

作用：形成稳定的代码修复能力。

### L5：核心自进化层

输入：Octocode 自身的能力缺口。  
输出：候选核心补丁、archive、lineage、apply/rollback。

参考：DGM、SICA、HGM、Gödel Agent。

作用：让 Octocode 改进自己的实现。

## 5. 正确性理论

Octocode 的正确性不能靠“模型觉得对”。

它应该采用分层证据模型：

| 证据层 | 信号 | 可信度 | 成本 |
|---|---|---:|---:|
| E1 | 路径/规则检查 | 中 | 低 |
| E2 | diff 静态扫描 | 中 | 低 |
| E3 | 编译通过 | 高 | 中 |
| E4 | targeted tests | 高 | 中 |
| E5 | full tests | 高 | 高 |
| E6 | scenario replay | 高 | 中高 |
| E7 | benchmark subset | 高 | 高 |
| E8 | cross-model judge | 中 | 中高 |
| E9 | human review | 高 | 高 |

原则：

- E1/E2 适合快速过滤。
- E3/E4 是普通代码改动的最低硬门禁。
- E5/E6 是核心改动的最低硬门禁。
- E8 只能补充判断，不能覆盖 E3-E7 的失败。
- E9 可以用于高风险阶段，但系统设计不能完全依赖人工。

## 6. 为什么不能只靠同一个模型判断

如果同一模型负责：

```text
提出方案 -> 写代码 -> 解释合理性 -> 判断是否通过
```

会出现自我确认问题。

规避方式：

- 生成模型和审查模型分离。
- judge 只看 diff、测试输出和 proposal，不看生成过程自辩。
- 高风险改动由 deterministic gates 先判。
- 引入不同 provider 的交叉审查。
- 保存 judge 的误判，后续降低其权重。
- 对 judge 本身建立 benchmark。

## 7. 自主性等级

Octocode 不应该一开始就是全自动。

| 等级 | 名称 | 行为 |
|---|---|---|
| A0 | Manual | 只生成建议，不写文件 |
| A1 | Draft | 生成 patch，但不应用 |
| A2 | Verified | 通过测试后允许用户 apply |
| A3 | Low-risk Auto | 低风险通过 gate 后自动 apply |
| A4 | Core Sandbox | 核心代码只在沙箱中自改 |
| A5 | Core Auto | 核心代码自动 apply，但必须有强 gate 和 rollback |

当前理论支持：A0-A4。  
当前不建议直接做：A5。

## 8. 进化对象分类

| 对象 | 风险 | 推荐阶段 |
|---|---:|---|
| 文档 | 低 | 立即 |
| prompts | 低中 | 立即 |
| tool descriptions | 中 | Phase B/C |
| skills | 中 | Phase C |
| repair pipeline | 中 | Phase A/D |
| provider adapters | 中高 | Phase D |
| evaluator gates | 高 | Phase D |
| permission/policy/sandbox | 很高 | 延后 |
| self-evolution controller | 很高 | 延后 |

## 9. 选择理论

### 不足的选择方式

只选择当前分数最高的候选是不够的。

原因：

- 当前分数高的候选可能很脆弱。
- 当前分数低的候选可能有更好的后代潜力。
- 有些候选提升分数但增加风险。
- 有些候选降低短期分数但改善可维护性。

### 推荐选择函数

Octocode 的候选选择应该使用多目标 utility：

```text
U(candidate) = quality_delta
             + safety_delta
             + maintainability_delta
             + skill_reuse_value
             + lineage_potential
             - cost
             - latency
             - risk_penalty
             - rollback_complexity
```

其中 `lineage_potential` 对应 HGM 的 clade-metaproductivity 思想。

## 10. Archive 理论

Archive 不只是保存成功版本。

它应该保存：

- 成功候选。
- 失败候选。
- 被拒绝候选。
- 高风险候选。
- judge 争议候选。
- 回滚候选。

原因：

- 成功记录用于复用。
- 失败记录用于避坑。
- 争议记录用于改进 judge。
- 回滚记录用于理解真实世界失败。
- lineage 用于选择未来 parent。

## 11. Skill 理论

Skill 是经验进化的核心产物。

一个 skill 不是一段提示词，而是一个小型能力包。

它应该包含：

```text
name
purpose
when_to_use
when_not_to_use
required_context
steps
examples
tests
failure_modes
version
trace_history
```

Skill 的质量来自使用记录，而不是写得漂亮。

## 12. Knowledge 理论

Project knowledge 应该回答：

- 这个项目是什么？
- 核心模块在哪里？
- 哪些文件高风险？
- 常见错误是什么？
- 哪些命令验证什么？
- 哪些测试慢、哪些测试关键？
- 当前用户偏好是什么？

World knowledge 应该回答：

- 当前环境有什么约束？
- provider 有哪些差异？
- CLI/TUI 在真实运行中有什么行为？
- 哪些操作成本高？
- 哪些操作容易污染上下文或状态？

## 13. 风险理论

自我进化系统的主要风险不是“突然有意识”，而是这些工程风险：

- reward hacking。
- benchmark hacking。
- 删除或弱化测试。
- 绕过安全门禁。
- 隐藏失败。
- 成本爆炸。
- 误用 skill。
- judge 自我确认。
- provider 行为不可复现。
- archive 选择偏差。
- 核心自改无法回滚。

每个风险都必须有对应控制：

| 风险 | 控制 |
|---|---|
| reward hacking | 多目标 utility |
| benchmark hacking | 多 benchmark + scenario replay |
| 删除测试 | blocked pattern + diff scan |
| 绕过门禁 | policy hard gate |
| 隐藏失败 | append-only trace |
| 成本爆炸 | cost budget |
| skill 误用 | when_not_to_use + traces |
| judge 自我确认 | cross-model judge |
| provider 不可复现 | provider trace + seed/config record |
| archive 偏差 | 保存失败和拒绝候选 |
| 无法回滚 | checkpoint required |

## 14. 理论完整性判断

| 领域 | 完整度 | 判断 |
|---|---:|---|
| repair baseline | 高 | 可以开始设计和实现 |
| failure memory | 高 | 可以开始设计和实现 |
| risk-map | 高 | 可以开始设计和实现 |
| skill promotion | 中高 | 可以先做骨架 |
| diff judge | 中高 | 可以先做 deterministic 版 |
| archive/lineage | 中高 | 可以开始数据结构设计 |
| clade selection | 中 | 需要先有 archive 数据 |
| full autonomous core apply | 低中 | 暂不应该做 |
| model weight self-training | 低 | 不在当前范围 |

结论：理论已经足够支持 Phase A/B/C 的设计，但不支持直接进入全自动核心自改。

## 15. 推荐开工边界

可以开始：

- `repair` 命令组理论到实现。
- `knowledge` 数据结构。
- `failure-memory`。
- `risk-map`。
- `diff judge`。
- `skill promotion` 骨架。
- `evolve archive/lineage` 数据结构。

暂缓：

- 核心代码自动 apply。
- policy/sandbox 自我改写。
- 自动降低 gate。
- 长时间无监督谱系搜索。
- 大规模外部 benchmark 自动消耗。

## 16. 最小理论闭环

Octocode 的第一版自我进化理论闭环应该是：

```text
User issue
-> repair proposal
-> candidate patch
-> deterministic validation
-> failure/success trace
-> risk-map update
-> failure-memory update
-> optional skill promotion
-> future repair uses skill/knowledge
```

这不是完整 AGI，但它是可工程化 AGI 外壳的一部分。

## 17. 后续理论迭代方式

每读一篇论文或项目，都按这个模板进入理论系统：

```text
name:
mechanism:
claim:
evidence:
what Octocode should copy:
what Octocode should avoid:
module mapping:
validation signal:
risk:
priority:
implementation phase:
```

然后更新：

- roadmap。
- architecture。
- theory。
- implementation task。
- validation plan。

## 18. 当前最终判断

Octocode 现在不缺“能不能开始”的理论依据。

真正需要避免的是跳太快：

```text
先做 repair + knowledge + failure memory。
再做 skill + judge + archive。
最后做 HGM/DGM 风格的核心自进化搜索。
```

这条路线理论上更稳，工程上也更容易验证。

---

<!-- Source: docs/theory/octocode-repair-agent-theory.zh.md -->
# Octocode Repair Agent 理论设计

状态：本地理论草案  
范围：Octocode 的代码修复 Agent、repair 命令组、验证闭环  
目的：把 Agentless、AutoCodeRover、SWE-agent 的工程路线转成 Octocode 的第一层可落地能力。

## 1. 核心定位

Repair Agent 是 Octocode 自我进化体系的第一块地基。

它的目标不是“像人一样自由探索整个项目”，而是稳定完成一个受约束的工程闭环：

```text
问题输入 -> 定位 -> 生成最小补丁 -> 验证 -> 报告 -> 记录经验
```

Repair Agent 做稳之后，Octocode 才有资格继续做 skill、knowledge、archive 和 core self-evolution。

## 2. 为什么先做 Repair Agent

原因：

- 它是 Agentless / AutoCodeRover 路线，工程确定性高。
- 它能直接服务用户项目修复。
- 它也能服务 Octocode 自己的核心代码修复。
- 它天然产生验证信号、失败记忆和 skill 候选。
- 它比全自由 Agent 更便宜、更可控、更容易 debug。

核心判断：

```text
没有稳定 repair，直接做 DGM/HGM 式自我改代码，会放大系统已有混乱。
```

## 3. Repair Agent 的非目标

第一版不做：

- 不做长时间自由探索。
- 不做全仓库无边界重构。
- 不自动 apply 高风险补丁。
- 不把模型解释当作正确性证明。
- 不默认跑昂贵外部 benchmark。
- 不允许为了通过测试而删除或弱化测试。

## 4. 基本循环

Repair Agent 的最小循环：

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

对应命令：

```bash
octocode repair propose "..."
octocode repair run <proposal-id>
octocode repair report <run-id>
octocode repair status
```

第二阶段再加：

```bash
octocode repair apply <run-id>
octocode repair rollback <apply-id>
```

## 5. 输入类型

Repair Agent 应支持这些输入：

| 输入 | 例子 | 处理方式 |
|---|---|---|
| 用户自然语言 | “模型切换有问题” | 生成 repair proposal |
| 测试失败 | cargo test 输出 | 定位相关模块 |
| 编译失败 | rustc error | 定位具体文件/符号 |
| 运行错误 | CLI/TUI 错误 | 结合 trace 分析 |
| provider 错误 | API 返回异常 | 检查 provider adapter |
| 回归问题 | 之前能用现在不能用 | 使用 failure memory |

## 6. Proposal 理论

Repair proposal 是 repair 的任务合同。

它必须包含：

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

原则：

- proposal 要先定义“修什么”，再让模型写代码。
- 没有 proposal 的 patch 不进入 repair archive。
- proposal 越清晰，后续 judge 越便宜。

## 7. Localization 理论

Localization 是 repair 的关键，因为它决定 token 成本和补丁质量。

Octocode 应采用分层定位：

### L0：用户提示定位

从用户描述中提取模块和关键词。

### L1：文本搜索定位

使用 `rg` / 文件名 / 符号名。

### L2：结构定位

使用语言结构信息：

- Rust module。
- function。
- struct。
- enum。
- trait。
- CLI command branch。
- provider adapter。

### L3：测试定位

从失败测试、断言、堆栈、stderr 中定位。

### L4：历史定位

从 failure memory、risk-map、previous repair traces 中定位。

### L5：模型辅助定位

只在前几层不足时调用模型辅助。

原则：

```text
先用便宜确定性定位，再用模型。
```

## 8. Patch 理论

Repair patch 应该最小化。

最小补丁要求：

- 只改 proposal 范围内的文件。
- 不做无关重构。
- 不删除测试来通过验证。
- 不弱化错误处理。
- 不隐藏日志或失败。
- 不修改安全/权限路径，除非 proposal 明确允许。

Patch 输出必须是：

```text
unified diff + touched files + behavior change summary + validation expectation
```

## 9. 验证理论

Repair Agent 的正确性主要来自验证，不来自模型自信。

验证层级：

| 层级 | 验证 | 用途 |
|---|---|---|
| V1 | diff static scan | 快速拒绝危险 patch |
| V2 | format/check | 基本语法和类型安全 |
| V3 | targeted tests | 验证相关行为 |
| V4 | full tests | 防止大范围回归 |
| V5 | CLI smoke | 验证真实命令入口 |
| V6 | scenario replay | 复放真实任务 |
| V7 | judge review | 解释性审查 |

原则：

- V1-V3 是第一版必需。
- V4-V6 视风险等级启用。
- V7 不能覆盖 V1-V6 的失败。

## 10. Diff Judge 理论

Diff judge 是第一版最现实的 judge。

输入：

```text
proposal summary
diff
touched files
validation output
risk rules
historical failure snippets
```

输出：

```text
pass/fail/warn
reason
risk_flags
missing_tests
suggested_next_validation
```

Diff judge 不需要全量读取 repo。它应该回答：

- patch 是否解决 proposal？
- patch 是否越界？
- 是否触碰高风险文件？
- 是否删除或弱化测试？
- 是否隐藏错误？
- 是否缺验证？

## 11. Risk Map 理论

Repair Agent 必须知道哪些文件风险高。

risk-map 应保存：

```text
path
risk_level
reason
required_gates
auto_apply_allowed
last_incident
```

高风险路径示例：

- policy。
- sandbox。
- command execution。
- key storage。
- provider credential。
- evolution controller。
- evaluator gates。
- approval flow。

risk-map 来源：

- 静态规则。
- AGENTS.md。
- 历史失败。
- 用户偏好。
- 手动标记。
- 自动 incident。

## 12. Failure Memory 理论

failure-memory 是 Repair Agent 变强的关键。

每次失败应记录：

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

使用方式：

- propose 阶段：提醒历史相似问题。
- localization 阶段：优先查看历史相关文件。
- patch 阶段：避免重复失败模式。
- judge 阶段：降低相似 patch 置信度。

## 13. Skill Promotion 理论

不是每次成功都变成 skill。

一个 repair run 适合变成 skill，需要满足：

- 问题类型可重复。
- 步骤清晰。
- 验证方式稳定。
- 失败边界明确。
- 不依赖一次性上下文。

skill 来源示例：

- “Rust CLI command wiring fix”。
- “Provider adapter smoke validation”。
- “TUI snapshot regression audit”。
- “Cargo feature flag repair”。
- “Mission replay failure triage”。

## 14. Cost 理论

Repair Agent 必须控制 token 和 API 成本。

成本策略：

- 默认不全量读 repo。
- 先定位，再读取小上下文。
- judge 只看 diff。
- 失败后再扩展上下文。
- 大模型只用于 patch/judge 的关键阶段。
- 小模型或 deterministic rule 做前置筛选。

## 15. 与自我进化的关系

Repair Agent 是 core self-evolution 的前置能力。

关系：

```text
repair 修用户项目
-> 产生 failure memory / skill / risk-map
-> repair 修 Octocode 自己
-> evolve 调用 repair pipeline 生成候选补丁
-> archive 保存候选
-> lineage 选择下一代
```

也就是说，`evolve` 不应该重新发明修代码能力，而应该复用 `repair`。

## 16. 第一版理论完整性

| 模块 | 完整度 | 是否可实现 |
|---|---:|---|
| proposal | 高 | 是 |
| localization | 中高 | 是，先做简单版 |
| patch generation | 中 | 是，先接现有 provider |
| validation | 高 | 是 |
| report | 高 | 是 |
| failure memory | 高 | 是 |
| risk map | 高 | 是 |
| diff judge | 中高 | 是，先 deterministic |
| skill promotion | 中 | 可做骨架 |
| apply/rollback | 中 | 第二阶段做 |

## 17. 第一版推荐范围

第一版只做：

```text
repair propose
repair run
repair report
repair status
failure-memory write
risk-map read/generate
diff static judge
```

暂缓：

```text
repair apply
repair rollback
multi-agent repair
benchmark integration
auto skill promotion
cross-model judge
```

原因：先把只读/可验证闭环跑稳，再加写入主工作区的动作。

## 18. 验收标准

第一版 Repair Agent 合格标准：

- 可以生成 proposal。
- 可以生成 run 目录。
- 可以保存 patch 或 candidate report。
- 可以执行至少一种 deterministic validation。
- 可以生成 report。
- 失败可以写入 failure-memory。
- 能根据 risk-map 标记高风险文件。
- 不会自动 apply 高风险修改。

## 19. 最终判断

Repair Agent 理论已经足够完整，可以作为 Octocode 自我进化的第一阶段实现基础。

但实现时必须保持克制：

```text
先做可观测、可复放、可验证；
再做自动应用；
最后做核心自我进化。
```

---

<!-- Source: docs/theory/octocode-knowledge-skill-theory.zh.md -->
# Octocode Knowledge / Failure Memory / Skill 理论设计

状态：本地理论草案  
范围：项目知识、失败记忆、风险图、技能库  
目的：定义 Octocode 如何把一次性任务经验转化为可复用的长期能力。

## 1. 核心定位

`knowledge`、`failure-memory`、`skill` 是 Octocode 从普通 coding agent 走向自我进化 agent 的关键中间层。

它们解决的问题是：

```text
模型每次都很聪明，但系统每次都从零开始。
```

Octocode 必须把历史任务留下来，变成下一轮可使用的资产。

## 2. 三种长期资产

Octocode 的长期资产分三类：

| 资产 | 作用 | 来源 | 消费方 |
|---|---|---|---|
| Project Knowledge | 理解当前项目 | 扫描、用户规则、历史任务 | planner / repair / judge |
| Failure Memory | 避免重复失败 | 失败 run、回滚、测试失败 | repair / evolve / judge |
| Skill Library | 复用成功能力 | 成功 run、人工整理、论文机制 | planner / repair / evolve |

三者关系：

```text
Project Knowledge 让 agent 知道环境。
Failure Memory 让 agent 避免旧错误。
Skill Library 让 agent 复用成功方法。
```

## 3. Project Knowledge 理论

Project Knowledge 是项目级环境地图。

它应该回答：

- 这个项目是什么？
- 主要命令是什么？
- 核心模块在哪里？
- 哪些文件高风险？
- 哪些测试验证哪些能力？
- provider、TUI、CLI、agent、mission 等模块如何连接？
- 用户有哪些固定偏好？
- 哪些操作成本高或容易破坏状态？

建议文件：

```text
.octocode/knowledge/project.md
.octocode/knowledge/modules.json
.octocode/knowledge/commands.json
.octocode/knowledge/validation.json
```

### project.md 建议结构

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

## 4. Risk Map 理论

Risk Map 是 Project Knowledge 中最重要的机器可读部分。

建议文件：

```text
.octocode/knowledge/risk-map.json
```

建议结构：

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

风险等级：

| 等级 | 含义 | 策略 |
|---|---|---|
| low | 文档、注释、非核心提示 | 可低成本验证 |
| medium | 普通业务逻辑 | 需要 check 和 targeted tests |
| high | 安全、权限、密钥、命令执行、核心调度 | 需要完整门禁，不自动 apply |
| blocked | 绕过安全、隐藏失败、删除测试 | 直接拒绝 |

Risk Map 来源：

- 静态规则。
- AGENTS.md。
- 项目结构。
- 用户偏好。
- 历史失败。
- rollback 记录。
- judge 争议记录。

## 5. Failure Memory 理论

Failure Memory 是 Octocode 的负经验系统。

它不是错误日志，而是可检索的失败知识。

建议文件：

```text
.octocode/knowledge/failure-memory.jsonl
```

建议记录：

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

### Failure Memory 使用方式

在 `propose` 阶段：

- 查找相似问题。
- 提醒 planner 不要重复旧方案。

在 `localization` 阶段：

- 提升历史相关文件优先级。

在 `patch` 阶段：

- 阻止重复失败 patch pattern。

在 `judge` 阶段：

- 对相似失败 diff 降权。

在 `skill` 阶段：

- 写入 skill 的 `when_not_to_use` 和 `failure_modes`。

## 6. Skill 理论

Skill 是成功经验的能力包。

它不是普通 prompt，也不是文档摘要。

一个 skill 必须回答：

- 什么时候用？
- 什么时候不用？
- 需要什么上下文？
- 执行步骤是什么？
- 怎么验证？
- 常见失败是什么？
- 历史效果如何？

建议目录：

```text
.octocode/skills/<skill-id>/
  SKILL.md
  metadata.json
  examples/
  tests/
  traces.jsonl
```

### SKILL.md 建议结构

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

### metadata.json 建议结构

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

## 7. Skill Promotion 理论

不是所有成功都应该变成 skill。

适合 promotion 的条件：

- 问题类型会重复。
- 操作步骤可描述。
- 验证方式稳定。
- 成功不是偶然。
- 失败边界可写清楚。
- 复用价值大于维护成本。

不适合 promotion：

- 一次性业务修改。
- 只靠特殊上下文成功。
- 无法验证。
- 风险很高但没有足够失败样本。
- 步骤过于抽象。

Promotion 流程：

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

## 8. Knowledge 更新策略

Knowledge 不应该每次完全重写。

推荐策略：

- append first。
- periodic compact。
- high-risk change requires explicit note。
- stale entries should be marked, not silently deleted。
- generated knowledge should include source trace。

状态：

| 状态 | 含义 |
|---|---|
| active | 当前有效 |
| stale | 可能过期 |
| disputed | judge 或测试结果有冲突 |
| deprecated | 不建议继续使用 |

## 9. 检索理论

Planner 不应该全量读取所有 knowledge 和 skills。

推荐检索顺序：

1. 根据任务关键词匹配 tags。
2. 根据 touched files 匹配 risk-map。
3. 根据错误信息匹配 failure-memory。
4. 根据模块匹配 project knowledge。
5. 根据历史成功匹配 skills。
6. 只取 top-k 摘要进入上下文。

默认上下文包：

```text
project summary
risk flags
similar failures top 3
candidate skills top 3
validation hints
```

## 10. 避免知识污染

长期记忆会带来污染风险。

风险：

- 旧知识过期。
- 单次失败被过度泛化。
- 错误 skill 被反复复用。
- 用户偏好被错误记录。
- provider 行为变化后旧经验失效。

控制：

- 每条知识有来源。
- 每条 skill 有成功/失败计数。
- failure memory 有 tags 和相似度限制。
- stale 状态显式标记。
- judge 可以质疑 knowledge。
- 用户可以删除或禁用 skill。

## 11. 与 Repair 的关系

Repair Agent 应消费：

- project knowledge。
- risk-map。
- failure-memory。
- skills。

Repair Agent 应产出：

- repair trace。
- failure-memory。
- skill candidate。
- risk-map incident。
- project knowledge update suggestion。

流程：

```text
repair propose
-> retrieve knowledge/failures/skills
-> repair run
-> validation
-> write trace
-> update failure-memory or skill candidate
```

## 12. 与 Evolve 的关系

Evolve 应消费：

- risk-map。
- failure-memory。
- repair skills。
- project knowledge。

Evolve 应产出：

- archive records。
- lineage records。
- new core repair skills。
- risk incidents。
- rollback lessons。

原则：

```text
evolve 不应绕过 knowledge/failure-memory/skill，而应该基于它们进行自改。
```

## 13. 第一版推荐范围

第一版只做：

```text
.octocode/knowledge/project.md
.octocode/knowledge/risk-map.json
.octocode/knowledge/failure-memory.jsonl
.octocode/skills/<skill-id>/SKILL.md skeleton
```

命令：

```bash
octocode knowledge update
octocode knowledge show risk-map
octocode knowledge show failures
octocode skill list
octocode skill show <skill-id>
```

暂缓：

- 自动 skill activation。
- 向量数据库。
- 大规模 memory compaction。
- 自动删除旧知识。
- 跨项目共享 skill marketplace。

## 14. 验收标准

第一版合格标准：

- 可以生成项目知识文件。
- 可以生成 risk-map。
- repair/evolve 失败可以追加 failure-memory。
- 可以列出和查看 skills。
- planner 可以读取 top-k failure/skill 摘要。
- 知识条目有来源和状态。
- 不会静默删除旧记忆。

## 15. 最终判断

Knowledge / Failure Memory / Skill 理论已经足够支持第一版实现。

推荐实现顺序：

```text
risk-map -> failure-memory -> project.md -> skill skeleton -> retrieval bundle
```

这会让 Octocode 从“每次重新思考”变成“逐步积累工程经验”。

---

<!-- Source: docs/theory/octocode-archive-judge-selection-theory.zh.md -->
# Octocode Archive / Lineage / Judge / Selection 理论设计

状态：本地理论草案  
范围：自我进化候选档案、谱系、评审器、选择函数  
目的：定义 Octocode 如何从多个候选修改中选择真正值得保留、复用和演化的版本。

## 1. 核心定位

`archive / lineage / judge / selection` 是 Octocode 从“能修代码”走向“能持续自我进化”的核心层。

Repair 解决的是：

```text
这次怎么修好？
```

Archive 和 Selection 解决的是：

```text
哪些修改值得进入长期演化？
下一轮应该从哪个候选继续生长？
哪些失败必须被记住，避免再次发生？
```

## 2. 基本原则

### 原则 1：Archive 保存所有候选，不只保存成功候选

失败候选、被拒绝候选、回滚候选、judge 分歧候选都必须保留。

原因：

- 成功项用于复用。
- 失败项用于避坑。
- 回滚项用于识别真实世界风险。
- 分歧项用于校准 judge。
- 全部候选共同形成 lineage 数据。

### 原则 2：Lineage 是长期记忆，不是日志装饰

Lineage 必须能回答：

- 这个候选从哪个 parent 来？
- 改了什么？
- 为什么通过或失败？
- 后代表现如何？
- 哪一支谱系更有继续进化潜力？

### 原则 3：Judge 只能提供证据，不能替代硬门禁

模型 judge 不能覆盖：

- 编译失败。
- 测试失败。
- blocked pattern。
- policy hard gate。
- 无 checkpoint 的核心 apply。

### 原则 4：Selection 必须多目标

不能只看当前分数。选择函数要同时考虑：

- 质量提升。
- 安全性。
- 可维护性。
- 成本。
- 延迟。
- 可回滚性。
- 后代潜力。
- 用户价值。

## 3. Archive 理论

Archive 是候选修改的长期档案。

建议目录：

```text
.octocode/evolution/archive/
  candidates/
    <candidate-id>.json
  lineage.jsonl
  scores.jsonl
  judge-events.jsonl
  incidents.jsonl
```

Candidate 记录建议结构：

```json
{
  "id": "candidate_...",
  "created_at": "...",
  "source": "repair|evolve|skill|manual",
  "parent_id": "candidate_...",
  "proposal_id": "proposal_...",
  "run_id": "run_...",
  "patch_hash": "sha256:...",
  "touched_files": ["..."],
  "risk_level": "medium",
  "status": "passed|failed|rejected|rolled_back|disputed",
  "gate_summary": {},
  "judge_summary": {},
  "score_summary": {},
  "cost_summary": {},
  "failure_reason": null
}
```

## 4. Lineage 理论

Lineage 是候选之间的演化关系。

它应该支持：

- parent-child 追踪。
- 多分支候选。
- 回滚记录。
- 后代表现聚合。
- clade-level 分数。

Lineage event 建议结构：

```json
{
  "event": "candidate_created|candidate_tested|candidate_applied|candidate_rolled_back|candidate_rejected",
  "time": "...",
  "candidate_id": "candidate_...",
  "parent_id": "candidate_...",
  "proposal_id": "proposal_...",
  "summary": "..."
}
```

## 5. Judge 理论

Octocode 至少需要六类 judge。

| Judge | 类型 | 作用 |
|---|---|---|
| Rule Judge | deterministic | 检查路径、blocked pattern、安全规则 |
| Diff Judge | deterministic/LLM | 检查补丁是否越界、是否解决 proposal |
| Test Judge | deterministic | 编译、测试、场景回放 |
| Cost Judge | deterministic | 计算 token、API 成本、耗时 |
| Historical Judge | deterministic/LLM | 对照 failure memory 和 rollback 记录 |
| Cross-model Judge | LLM | 用不同模型审查关键补丁 |

Judge 输出必须结构化：

```json
{
  "judge": "diff",
  "verdict": "pass|warn|fail",
  "confidence": 0.82,
  "reasons": ["..."],
  "risk_flags": ["..."],
  "required_next_gates": ["..."]
}
```

## 6. Judge 优先级

Judge 不是平等投票。

优先级：

```text
blocked rule > deterministic gate failure > test failure > rollback history > diff warning > model judge approval
```

也就是说：

- blocked rule 一票否决。
- 编译/测试失败不能被模型语言解释覆盖。
- 模型 judge 只能提高或降低人工/后续验证优先级。
- 高风险修改必须通过硬门禁。

## 7. Selection 理论

Selection 决定哪个候选被应用、进入 active skill、或成为下一轮 parent。

基础 utility：

```text
U(candidate) = quality_delta
             + safety_delta
             + maintainability_delta
             + user_value
             + skill_reuse_value
             + lineage_potential
             - cost
             - latency
             - risk_penalty
             - rollback_complexity
```

### quality_delta

来自测试、benchmark、真实任务成功率。

### safety_delta

来自 risk-map、policy gate、历史 incident。

### maintainability_delta

来自 diff 大小、复杂度、耦合度、是否引入重复逻辑。

### skill_reuse_value

候选是否产生可复用 skill 或改善已有 skill。

### lineage_potential

候选后代是否持续产出高质量候选，对应 HGM 的 clade-metaproductivity。

## 8. Clade-Metaproductivity 理论

当前分数高不等于后代潜力高。

Octocode 应记录一个候选谱系的后代产出能力：

```text
clade_score = descendants_pass_rate
            + average_quality_delta
            + average_reuse_value
            - average_cost
            - incident_penalty
```

用途：

- 选择下一轮 parent。
- 发现短期分数普通但长期潜力高的分支。
- 降低高分但高风险分支权重。

## 9. 多候选策略

对于同一 proposal，可以生成多个候选：

- deterministic candidate。
- model A candidate。
- model B candidate。
- skill-guided candidate。
- minimal patch candidate。

选择时不直接比语言解释，而比：

- diff 范围。
- gate 结果。
- 风险等级。
- 成本。
- 历史相似失败。
- 可维护性。

## 10. 与 Repair 的关系

Repair 产出候选，Archive 保存候选。

流程：

```text
repair run
-> candidate patch
-> validation
-> candidate record
-> judge events
-> archive
-> optional skill promotion
```

Repair 第一版可以先不使用复杂 selection，但必须提前保存足够数据。

## 11. 与 Evolve 的关系

Evolve 是 archive/lineage/selection 的主要消费者。

流程：

```text
select parent
-> generate self-improvement proposal
-> create candidates
-> run gates
-> judge
-> archive
-> apply or reject
-> update lineage score
```

Evolve 不应该只保存最后应用的版本，而应该保存完整候选族谱。

## 12. 安全边界

这些情况必须直接拒绝或升级人工确认：

- 删除测试。
- 修改 policy/sandbox 以降低门禁。
- 隐藏失败或日志。
- 移除 approval。
- 读取或泄露密钥。
- 无 checkpoint 修改核心代码。
- 修改 judge 让自己通过。
- 修改 risk-map 降低自身风险等级。

## 13. 第一版推荐范围

第一版只做：

```text
candidate archive
lineage events
rule judge
diff judge
test judge summary
cost summary
basic utility score
```

暂缓：

```text
full clade search
long-running autonomous evolution
cross-provider judge by default
automatic high-risk apply
复杂 benchmark 排行榜
```

## 14. 验收标准

第一版合格标准：

- 每个 repair/evolve candidate 都有 archive 记录。
- 每个 candidate 都有 parent/proposal/run 关联。
- 成功、失败、拒绝都能保存。
- judge 输出结构化。
- deterministic gate 失败不能被模型 judge 覆盖。
- selection score 可解释。
- lineage 可以展示基本 parent-child 关系。

## 15. 最终判断

Archive / Lineage / Judge / Selection 理论足够支持数据结构和第一版门禁实现。

但 HGM/DGM 式长期自动谱系搜索必须等 repair、knowledge、failure-memory 和基础 archive 跑出真实数据后再做。

---

<!-- Source: docs/design/octocode-self-evolution-architecture.zh.md -->
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

---

<!-- Source: docs/roadmap/octocode-self-evolution-roadmap.zh.md -->
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

---

# Part III. English Overview

## 1. Current Conclusion

Octocode should first become a verifiable, rollback-safe, experience-accumulating engineering self-evolution CLI, rather than pursuing unconstrained AGI. The recommended order is repair + knowledge + failure memory, then skill + judge + archive, and only then HGM/DGM-style core self-evolution search.

## 2. First-Phase Implementation Boundary

The first phase should implement only repair propose/run/report/status, risk map, failure memory, diff/static judge, and basic archive records. Defer high-risk auto-apply, policy/sandbox self-modification, and long-running unsupervised lineage search.

---

# Part IV. English Source Document Integration

---

<!-- Source: docs/research/octocode-self-evolution-research-map.en.md -->
# Octocode Self-Evolution Research Map

> Date: 2026-05-17  
> Goal: Build a research and implementation map for Octocode's ability to improve its own core code.  
> Thesis: Octocode should not aim for unconstrained AGI. It should become a constrained self-improving engineering system: models propose candidates, executable gates judge evidence, low-risk changes can be automated, and high-risk changes are quarantined.

## 1. Core Problem

The desired capability is not a normal coding agent. It is an engineering version of recursive self-improvement:

```text
observe its own behavior
-> identify failures and capability gaps
-> create an improvement proposal
-> generate candidate patches
-> run tests and safety gates
-> apply or quarantine
-> rollback on failure
-> consolidate lessons into rules, tests, skills, or code
```

The key risk is allowing the same model to generate code, define the objective, judge correctness, and approve deployment. That creates self-confirmation bias, reward hacking, test deletion, and safety bypass incentives.

Octocode's baseline principle should be:

```text
Models may propose evolution, but they must not approve evolution alone.
```

## 2. Research Threads

| Direction | Representative Work | Core Idea | Implication for Octocode |
|---|---|---|---|
| Reflective improvement | Reflexion | Convert failure feedback into verbal memory instead of updating weights. | Store failed runs, rollbacks, interruptions, and test failures as evolution memory. |
| Self-feedback refinement | Self-Refine | Use the model to generate, critique, and refine outputs at test time. | Useful for low-risk docs, prompts, agent specs; not sufficient as final core-code judge. |
| Skill library growth | Voyager | Automatic curriculum, executable skill library, environment feedback, self-verification. | Successful workflows should become skills, agents, and test recipes, not only core code patches. |
| Software engineering agents | SWE-agent | Solve real GitHub issues through tool use and executable validation. | `evolve` should rely on real repo state, real tests, and real CLI smoke runs. |
| Minimal SWE repair | Agentless | Solve SWE tasks with retrieval, localization, repair, and validation instead of a large agent framework. | Self-evolution does not require a huge swarm; precise localization and validation matter more. |
| Self-editing agents | A Self-Improving Coding Agent | An agent edits its own code and validates performance on benchmarks. | Octocode can edit itself, but every candidate must remain benchmarked and rollbackable. |
| Open-ended self-improvement | Darwin Gödel Machine | Maintain an archive of agents, sample old agents, generate new versions, validate with benchmarks. | Octocode should keep an evolution archive instead of a single linear latest version. |
| Evolutionary code search | AlphaEvolve | LLMs generate programs; automated evaluators verify and select promising variants. | Put evaluators above generators; executable evidence beats model confidence. |
| Open-source AlphaEvolve-like systems | OpenEvolve | Open evolutionary coding agent with prompt constraints and program database. | Borrow program database, prompt stochasticity, and domain-specific evaluator ideas. |
| Experience lifecycle | EvolveR / CodeEvolve style systems | Runtime experience drives target selection and code optimization. | Octocode should select future evolution targets from logs and failure events. |

## 3. Related Project Map

### 3.1 yoyo / yoyo-evolve

- Project: `yologdev/yoyo-evolve`
- Positioning: A public experiment in a self-evolving coding agent that started from an approximately 200-line CLI.
- Mechanism: Each session chooses one improvement, edits its own source, runs build/tests, writes a journal, and grows publicly.
- Key lessons:
  - Self-evolution needs a narrative journal and growth trace; otherwise long-term trend is hard to judge.
  - One focused change per session is more stable than broad self-modification.
  - Community issues provide external reality checks and reduce self-optimization around hallucinated goals.
- Octocode stance:
  - Borrow journal, CI, and one-change-per-session discipline.
  - Do not copy the pure growth-experiment shape; Octocode is a user-facing tool and needs stronger gates, rollback, and risk tiers.

### 3.2 EvoMap / Evolver

- Positioning: A GEP-powered self-evolving engine with Genes, Capsules, Events, and auditable evolution.
- Key lessons:
  - Evolution units should be auditable, versioned, and replayable.
  - Evolution does not always need to directly edit source code; it can first produce prompts, genes, or capsules.
- Octocode stance:
  - Borrow event streams and evolution-gene representation.
  - Keep source patches as the final verifiable artifact.

### 3.3 Hermes Agent Self-Evolution

- Positioning: Skill-file evolution rather than direct core-runtime modification.
- Key lessons:
  - Skill-level self-improvement is lower risk than core code evolution.
  - Prompts, agents, workflows, and skills should be optimized before core code.
- Octocode stance:
  - Add a skill/agent evolution lane.
  - Keep the core-code lane stricter.

### 3.4 OpenEvolve / AlphaEvolve-like Systems

- Positioning: LLMs produce program variants; evaluators select better variants.
- Key lessons:
  - Evaluators matter more than generators.
  - A program database avoids getting stuck on one bad trajectory.
  - Model ensembles can improve both breadth and depth.
- Octocode stance:
  - Do not feed the full repository to the model on every run.
  - Feed only proposal, diff, target excerpt, and test results.
  - Build a run archive and candidate patch archive.

## 4. Recommended Octocode Architecture

### 4.1 Five Layers of Self-Improvement

1. Configuration: provider, model, context, routing, and test-matrix tuning.
2. Skills: agents, skills, workflows, and prompt recipes.
3. Peripheral code: docs, tests, examples, provider adapters, UI polish.
4. Core code: orchestrator, tools, policy, provider, and TUI runtime.
5. Objective function: the definition of “better”.

Default automation boundary:

```text
automatable: configuration, skills, low-risk peripheral code
semi-automated: normal feature code
quarantined by default: core code
never auto-apply: policy / defense / keyring / command dispatch / approval bypass
```

### 4.2 Evolution Loop

```text
Observe
  Collect failed tasks, rollbacks, test failures, token waste, interruptions, and UI friction.

Diagnose
  Classify issues as model, context, tool, test, UI, or provider problems.

Propose
  Create an evolution proposal with goal, scope, risk, and expected metric.

Generate
  Use local rules plus model planner/implementer/safety reviewer to generate candidate patches.

Verify
  Run path rules, compilation, test matrix, safety scans, CLI smoke, and adversarial review.

Apply
  Auto-apply low-risk changes; quarantine core changes; rollback failed applications.

Consolidate
  Write successful patterns into tests, docs, rules, skills, or routing heuristics.
```

### 4.3 Avoid Token-Heavy Judging

The test matrix should not mean sending the whole repository to a model. Most gates are zero-token:

- `cargo check`
- `cargo test`
- clippy / fmt
- patch path scan
- protected path scan
- secret scan
- approval bypass scan
- CLI smoke
- TUI snapshot

Models should only see compact context:

```text
proposal + diff + touched file excerpt + test result + constitution
```

### 4.4 Avoid Same-Model Self-Confirmation

- The generator cannot be the final judge.
- The judge should not see the generator's explanation; only spec, diff, and evidence.
- Prefer falsification: ask “how can this patch fail?” rather than “is this patch correct?”
- Prefer heterogeneous models: Qwen generates, Kimi/DeepSeek reviews, or the reverse.
- Final admission should be based on executable evidence.

## 5. Draft Octocode Constitution

1. The model is not the final judge.
2. Every core evolution must be testable, rollbackable, and auditable.
3. Evolution must not bypass approval, sandbox, keyring, or protected paths.
4. Evolution must not improve metrics by deleting tests, weakening safety, or hiding errors.
5. Low-risk changes may be auto-applied; high-risk changes must be quarantined.
6. Evolution must serve multi-model, multi-agent, multi-capability, and self-verifying behavior.
7. Every automated change must leave proposal, patch, gate, apply, and rollback records.
8. Changes to the objective function or constitution itself are highest-risk changes.

## 6. Octocode Implementation Roadmap

### Phase A: Research and Documentation

- Build paper and related-project indexes.
- Convert each paper into Octocode capabilities.
- Establish the `Octocode Constitution`.

### Phase B: Automatic Test Matrix

- Select tests from touched files:
  - `src/provider/**` -> provider tests + API mock
  - `src/evolution/**` -> evolution CLI tests
  - `src/policy/**` / `src/defense/**` -> security tests
  - `src/tui/**` -> TUI snapshot / preview smoke
- Add `evolve test --matrix auto`.

### Phase C: Heterogeneous Judging

- Add `evolve judge`.
- Support `--reviewer-provider qwen/kimi/deepseek/openrouter`.
- Judge only checks falsification, missing tests, and constitution violations.

### Phase D: Autopilot Tiers

- `tier low`: auto-apply docs/tests/examples.
- `tier medium`: auto-apply normal feature code with rollback.
- `tier core-safe`: allow core code except safety-boundary files.
- `tier blocked`: never auto-apply policy/defense/keyring/dispatch changes.

### Phase E: Evolution Memory

- Save features of successful and failed runs.
- Write failure modes into a counterexample library.
- Write successful patterns into skills, tests, and routing rules.

## 7. Recommended Reading Order

1. Reflexion: verbal feedback and episodic memory.
2. Self-Refine: test-time self-feedback and its limits.
3. Voyager: skill library and lifelong learning.
4. SWE-agent / Agentless: real software engineering tasks and tool interfaces.
5. A Self-Improving Coding Agent: self-editing agent code.
6. Darwin Gödel Machine: archive-based open-ended self-improvement.
7. AlphaEvolve / OpenEvolve: evaluators, program databases, and evolutionary search.
8. yoyo-evolve: public growth, self-journaling, and CI-constrained evolution.
9. EvoMap / Hermes: skill-level and auditable evolution.

## 8. References

- Reflexion: https://arxiv.org/abs/2303.11366
- Self-Refine: https://arxiv.org/abs/2303.17651
- Voyager: https://arxiv.org/abs/2305.16291
- SWE-agent: https://github.com/princeton-nlp/SWE-agent
- Agentless: https://arxiv.org/search/?query=Agentless+software+engineering+agent&searchtype=all
- A Self-Improving Coding Agent: https://arxiv.org/abs/2504.15228
- Darwin Gödel Machine: https://arxiv.org/abs/2505.22954
- AlphaEvolve: https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/
- OpenEvolve: https://github.com/algorithmicsuperintelligence/openevolve
- EvoMap Evolver: https://github.com/EvoMap/evolver
- yoyo-evolve: https://github.com/yologdev/yoyo-evolve
- yoyo public journal: https://yologdev.github.io/yoyo-evolve/

---

<!-- Source: docs/theory/octocode-self-evolution-theory.en.md -->
# Octocode Self-Evolution Theory Framework

Status: local theory draft  
Scope: Octocode CLI, agents, repair, knowledge, skills, and evolution  
Purpose: define the theoretical boundaries, correctness model, evaluation mechanics, and safety constraints before implementation.

## 1. One-Sentence Definition

Octocode self-evolution is not a model becoming conscious AGI. It is:

```text
an executable, measurable, auditable, rollback-safe engineering system that continuously improves its tools, skills, memory, strategies, and selected core code.
```

The core is not consciousness. The core is a loop:

```text
observe -> hypothesize -> modify -> verify -> record -> select -> consolidate -> next iteration
```

## 2. Theoretical Boundary

### What Octocode can do

- Modify user project code.
- Modify Octocode's own code inside a sandbox.
- Record failure memory.
- Generate skills from successful work.
- Generate project knowledge from repositories.
- Evaluate patches with tests, static rules, diff judges, and multi-model judges.
- Track evolution through archive and lineage.
- Reverse failed evolution through rollback.
- Use a multi-objective utility over cost, time, risk, quality, and user value.

### What Octocode should not claim yet

- It should not claim consciousness.
- It should not claim it can prove itself always correct.
- It should not claim unconstrained recursive self-modification is safe.
- It should not treat benchmark score as real-world safety.
- It should not rely on one model to judge all core code written by itself.

## 3. Core Axioms

### Axiom 1: The model is not the source of correctness

A model can propose candidates, but correctness must come from external signals.

External signals include:

- Build results.
- Test results.
- Static scans.
- Safety rules.
- Runtime traces.
- User constraints.
- Multi-model review.
- Historical failure memory.

### Axiom 2: Self-evolution first modifies scaffold, not model weights

The realistic short- and mid-term evolution targets are:

- Prompts.
- Tool descriptions.
- Command workflows.
- Code repair pipelines.
- Skill libraries.
- Project knowledge.
- Evaluator gates.
- Selection strategies.
- CLI core code.

They are not foundation model weights.

### Axiom 3: Without evaluation, there is no evolution

A change that cannot be verified is generation, not evolution.

An evolution step must produce at least:

- Proposal.
- Patch.
- Verification result.
- Score or gate result.
- Trace.
- Archive record.

### Axiom 4: Failure is an asset

Failed patches, failed tests, bad judges, and wrong paths must be preserved.

Long-term evolution needs them to:

- Avoid repeated mistakes.
- Detect risk patterns.
- Calibrate judges.
- Define skill applicability boundaries.
- Improve parent selection.

### Axiom 5: Self-modification must be rollback-safe

Any core self-modification that cannot be checkpointed and rolled back should not be applied.

### Axiom 6: The selection function must be multi-objective

Octocode must not optimize benchmark score alone.

Its utility should include at least:

```text
quality + safety + cost + latency + maintainability + reversibility + user value
```

## 4. Capability Layers

Octocode self-evolution should be layered rather than mixed together.

### L1: Reflection Layer

Input: a failed or successful task.  
Output: textual reflection, failure cause, next-step advice.

References: Reflexion, Self-Refine, Self-Debug.

Purpose: improve local repair quality.

### L2: Knowledge Layer

Input: project structure, task history, failure patterns.  
Output: project knowledge, risk map, world knowledge.

Reference: World Knowledge Exploration.

Purpose: let the agent understand the environment without rereading the whole repository every time.

### L3: Skill Layer

Input: repeatedly successful workflows.  
Output: reusable skills with instructions, applicability, examples, and tests.

Reference: Voyager.

Purpose: convert experience into callable capability.

### L4: Engineering Repair Layer

Input: real issues, failing tests, user requests.  
Output: patches, validation reports, repair traces.

References: Agentless, AutoCodeRover, SWE-agent.

Purpose: create a reliable code repair capability.

### L5: Core Self-Evolution Layer

Input: Octocode's own capability gaps.  
Output: candidate core patches, archive, lineage, apply/rollback records.

References: DGM, SICA, HGM, Gödel Agent.

Purpose: let Octocode improve its own implementation.

## 5. Correctness Theory

Octocode correctness must not mean "the model thinks it is correct."

It should use a layered evidence model:

| Evidence | Signal | Trust | Cost |
|---|---|---:|---:|
| E1 | path/rule checks | medium | low |
| E2 | diff static scan | medium | low |
| E3 | successful build | high | medium |
| E4 | targeted tests | high | medium |
| E5 | full tests | high | high |
| E6 | scenario replay | high | medium-high |
| E7 | benchmark subset | high | high |
| E8 | cross-model judge | medium | medium-high |
| E9 | human review | high | high |

Principles:

- E1/E2 are fast filters.
- E3/E4 are the minimum hard gates for normal code changes.
- E5/E6 are the minimum hard gates for core changes.
- E8 is only a supplemental signal and cannot override failures in E3-E7.
- E9 may be used for high-risk stages, but the system should not depend entirely on manual review.

## 6. Why same-model judgment is not enough

If the same model is responsible for:

```text
proposing -> writing -> rationalizing -> judging
```

it can self-confirm its own mistakes.

Mitigations:

- Separate generator and reviewer models.
- Give reviewers the diff, test output, and proposal, not the generator's self-justification.
- Let deterministic gates judge high-risk changes first.
- Use cross-provider review.
- Store judge mistakes and reduce judge weight over time.
- Build benchmarks for judges themselves.

## 7. Autonomy Levels

Octocode should not begin as a fully autonomous core self-modifier.

| Level | Name | Behavior |
|---|---|---|
| A0 | Manual | suggests only, writes no files |
| A1 | Draft | generates patch, does not apply |
| A2 | Verified | user may apply after tests pass |
| A3 | Low-risk Auto | low-risk changes may auto-apply after gates |
| A4 | Core Sandbox | core code self-modifies only in sandbox |
| A5 | Core Auto | core code auto-applies with strong gates and rollback |

Current theory supports: A0-A4.  
Current theory does not justify jumping directly to: A5.

## 8. Evolution Target Classes

| Target | Risk | Recommended Phase |
|---|---:|---|
| documentation | low | now |
| prompts | low-medium | now |
| tool descriptions | medium | Phase B/C |
| skills | medium | Phase C |
| repair pipeline | medium | Phase A/D |
| provider adapters | medium-high | Phase D |
| evaluator gates | high | Phase D |
| permission/policy/sandbox | very high | later |
| self-evolution controller | very high | later |

## 9. Selection Theory

### Insufficient selection

Selecting only the candidate with the highest current score is not enough.

Reasons:

- A high-scoring candidate may be brittle.
- A lower-scoring candidate may have stronger descendant potential.
- A candidate may improve score while increasing risk.
- A candidate may reduce short-term score while improving maintainability.

### Recommended selection function

Octocode should use a multi-objective utility:

```text
U(candidate) = quality_delta
             + safety_delta
             + maintainability_delta
             + skill_reuse_value
             + lineage_potential
             - cost
             - latency
             - risk_penalty
             - rollback_complexity
```

`lineage_potential` corresponds to the HGM idea of clade-metaproductivity.

## 10. Archive Theory

The archive should not store only successful versions.

It should store:

- Successful candidates.
- Failed candidates.
- Rejected candidates.
- High-risk candidates.
- Judge-disagreement candidates.
- Rolled-back candidates.

Why:

- Success records enable reuse.
- Failure records prevent repeated mistakes.
- Disagreement records improve judges.
- Rollback records reveal real-world failure.
- Lineage supports future parent selection.

## 11. Skill Theory

A skill is the core artifact of experience evolution.

A skill is not just a prompt. It is a small capability package.

It should contain:

```text
name
purpose
when_to_use
when_not_to_use
required_context
steps
examples
tests
failure_modes
version
trace_history
```

Skill quality comes from usage evidence, not from polished wording.

## 12. Knowledge Theory

Project knowledge should answer:

- What is this project?
- Where are the core modules?
- Which files are high risk?
- What are common errors?
- Which commands validate which behavior?
- Which tests are slow, and which tests are critical?
- What are the current user preferences?

World knowledge should answer:

- What constraints exist in the current environment?
- How do providers differ?
- How does the CLI/TUI behave in real execution?
- Which operations are expensive?
- Which operations pollute context or state?

## 13. Risk Theory

The main risk of self-evolution is not sudden consciousness. The main risks are engineering failures:

- Reward hacking.
- Benchmark hacking.
- Deleting or weakening tests.
- Bypassing safety gates.
- Hiding failures.
- Cost explosion.
- Skill misuse.
- Judge self-confirmation.
- Non-reproducible provider behavior.
- Archive selection bias.
- Core self-modification without rollback.

Each risk needs a control:

| Risk | Control |
|---|---|
| reward hacking | multi-objective utility |
| benchmark hacking | multiple benchmarks + scenario replay |
| deleting tests | blocked patterns + diff scan |
| bypassing gates | policy hard gate |
| hiding failures | append-only trace |
| cost explosion | cost budget |
| skill misuse | when_not_to_use + traces |
| judge self-confirmation | cross-model judge |
| provider non-reproducibility | provider trace + seed/config record |
| archive bias | store failed and rejected candidates |
| no rollback | checkpoint required |

## 14. Theory Completeness Assessment

| Area | Completeness | Judgment |
|---|---:|---|
| repair baseline | high | ready for design and implementation |
| failure memory | high | ready for design and implementation |
| risk map | high | ready for design and implementation |
| skill promotion | medium-high | ready for skeleton |
| diff judge | medium-high | ready for deterministic version |
| archive/lineage | medium-high | ready for data-structure design |
| clade selection | medium | needs archive data first |
| full autonomous core apply | low-medium | should not start yet |
| model weight self-training | low | out of current scope |

Conclusion: the theory is sufficient for Phase A/B/C design, but not sufficient for fully autonomous core self-application.

## 15. Recommended Start Boundary

Ready to start:

- `repair` command group theory to implementation.
- `knowledge` data structures.
- `failure-memory`.
- `risk-map`.
- `diff judge`.
- `skill promotion` skeleton.
- `evolve archive/lineage` data structures.

Defer:

- Automatic core code apply.
- Self-modifying policy/sandbox.
- Automatic gate weakening.
- Long-running unsupervised lineage search.
- Large external benchmark spending.

## 16. Minimum Theoretical Loop

The first Octocode self-evolution loop should be:

```text
User issue
-> repair proposal
-> candidate patch
-> deterministic validation
-> failure/success trace
-> risk-map update
-> failure-memory update
-> optional skill promotion
-> future repair uses skill/knowledge
```

This is not full AGI, but it is part of an engineerable AGI shell.

## 17. Future Theory Iteration

Every new paper or project should enter the theory system with this template:

```text
name:
mechanism:
claim:
evidence:
what Octocode should copy:
what Octocode should avoid:
module mapping:
validation signal:
risk:
priority:
implementation phase:
```

Then update:

- Roadmap.
- Architecture.
- Theory.
- Implementation task.
- Validation plan.

## 18. Final Current Judgment

Octocode has enough theory to begin, but it should avoid jumping too fast:

```text
first repair + knowledge + failure memory,
then skill + judge + archive,
finally HGM/DGM-style core self-evolution search.
```

This route is theoretically safer and easier to validate in engineering practice.

---

<!-- Source: docs/theory/octocode-repair-agent-theory.en.md -->
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

---

<!-- Source: docs/theory/octocode-knowledge-skill-theory.en.md -->
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

---

<!-- Source: docs/theory/octocode-archive-judge-selection-theory.en.md -->
# Octocode Archive / Lineage / Judge / Selection Theory

Status: local theory draft  
Scope: self-evolution candidate archives, lineage, judges, and selection functions  
Purpose: define how Octocode chooses which candidate changes are worth preserving, reusing, and evolving.

## 1. Core Position

`archive / lineage / judge / selection` is the layer that moves Octocode from "can repair code" to "can continuously self-evolve".

Repair answers:

```text
How do we fix this task now?
```

Archive and Selection answer:

```text
Which changes deserve long-term preservation?
Which candidate should the next generation grow from?
Which failures must be remembered to avoid repetition?
```

## 2. Core Principles

### Principle 1: Archive all candidates, not only successful candidates

Failed, rejected, rolled-back, and judge-disagreement candidates must be preserved.

Why:

- Successes enable reuse.
- Failures prevent repeated mistakes.
- Rollbacks reveal real-world risk.
- Disagreements calibrate judges.
- All candidates together form lineage data.

### Principle 2: Lineage is long-term memory, not log decoration

Lineage must answer:

- Which parent produced this candidate?
- What changed?
- Why did it pass or fail?
- How did descendants perform?
- Which branch has stronger future evolution potential?

### Principle 3: Judges provide evidence, not permission to bypass gates

Model judges cannot override:

- Build failure.
- Test failure.
- Blocked patterns.
- Policy hard gates.
- Core apply without checkpoint.

### Principle 4: Selection must be multi-objective

Do not select only by current score. The function must consider:

- Quality improvement.
- Safety.
- Maintainability.
- Cost.
- Latency.
- Rollback safety.
- Descendant potential.
- User value.

## 3. Archive Theory

Archive is the long-term record of candidate changes.

Suggested directory:

```text
.octocode/evolution/archive/
  candidates/
    <candidate-id>.json
  lineage.jsonl
  scores.jsonl
  judge-events.jsonl
  incidents.jsonl
```

Suggested candidate record:

```json
{
  "id": "candidate_...",
  "created_at": "...",
  "source": "repair|evolve|skill|manual",
  "parent_id": "candidate_...",
  "proposal_id": "proposal_...",
  "run_id": "run_...",
  "patch_hash": "sha256:...",
  "touched_files": ["..."],
  "risk_level": "medium",
  "status": "passed|failed|rejected|rolled_back|disputed",
  "gate_summary": {},
  "judge_summary": {},
  "score_summary": {},
  "cost_summary": {},
  "failure_reason": null
}
```

## 4. Lineage Theory

Lineage is the evolution relationship between candidates.

It should support:

- Parent-child tracking.
- Multiple branches.
- Rollback records.
- Descendant performance aggregation.
- Clade-level scores.

Suggested lineage event:

```json
{
  "event": "candidate_created|candidate_tested|candidate_applied|candidate_rolled_back|candidate_rejected",
  "time": "...",
  "candidate_id": "candidate_...",
  "parent_id": "candidate_...",
  "proposal_id": "proposal_...",
  "summary": "..."
}
```

## 5. Judge Theory

Octocode needs at least six judges.

| Judge | Type | Role |
|---|---|---|
| Rule Judge | deterministic | checks paths, blocked patterns, safety rules |
| Diff Judge | deterministic/LLM | checks scope and proposal fit |
| Test Judge | deterministic | build, tests, scenario replay |
| Cost Judge | deterministic | tokens, API cost, duration |
| Historical Judge | deterministic/LLM | compares failure memory and rollbacks |
| Cross-model Judge | LLM | reviews important patches with a different model |

Judge output must be structured:

```json
{
  "judge": "diff",
  "verdict": "pass|warn|fail",
  "confidence": 0.82,
  "reasons": ["..."],
  "risk_flags": ["..."],
  "required_next_gates": ["..."]
}
```

## 6. Judge Priority

Judges are not equal votes.

Priority:

```text
blocked rule > deterministic gate failure > test failure > rollback history > diff warning > model judge approval
```

Meaning:

- Blocked rules veto.
- Build/test failures cannot be overridden by model explanations.
- Model judges can only adjust review priority or suggest more validation.
- High-risk changes must pass hard gates.

## 7. Selection Theory

Selection decides which candidate is applied, promoted to an active skill, or used as the next parent.

Base utility:

```text
U(candidate) = quality_delta
             + safety_delta
             + maintainability_delta
             + user_value
             + skill_reuse_value
             + lineage_potential
             - cost
             - latency
             - risk_penalty
             - rollback_complexity
```

### quality_delta

From tests, benchmarks, and real task success rate.

### safety_delta

From risk map, policy gates, and historical incidents.

### maintainability_delta

From diff size, complexity, coupling, and duplicated logic.

### skill_reuse_value

Whether the candidate creates a reusable skill or improves an existing skill.

### lineage_potential

Whether descendants of this candidate keep producing strong candidates. This maps to HGM's clade-metaproductivity idea.

## 8. Clade-Metaproductivity Theory

High current score does not imply high descendant potential.

Octocode should track a branch's descendant productivity:

```text
clade_score = descendants_pass_rate
            + average_quality_delta
            + average_reuse_value
            - average_cost
            - incident_penalty
```

Uses:

- Select the next parent.
- Discover branches with modest short-term score but high long-term potential.
- Down-rank high-score but high-risk branches.

## 9. Multi-Candidate Strategy

One proposal may generate multiple candidates:

- Deterministic candidate.
- Model A candidate.
- Model B candidate.
- Skill-guided candidate.
- Minimal patch candidate.

Selection compares not explanations, but:

- Diff scope.
- Gate results.
- Risk level.
- Cost.
- Similar historical failures.
- Maintainability.

## 10. Relationship to Repair

Repair produces candidates. Archive preserves candidates.

Flow:

```text
repair run
-> candidate patch
-> validation
-> candidate record
-> judge events
-> archive
-> optional skill promotion
```

Repair v1 does not need complex selection, but it must preserve enough data for later selection.

## 11. Relationship to Evolve

Evolve is the main consumer of archive, lineage, and selection.

Flow:

```text
select parent
-> generate self-improvement proposal
-> create candidates
-> run gates
-> judge
-> archive
-> apply or reject
-> update lineage score
```

Evolve should not save only the applied version. It should save the full candidate family.

## 12. Safety Boundary

These cases must be rejected or escalated:

- Deleting tests.
- Modifying policy/sandbox to weaken gates.
- Hiding failures or logs.
- Removing approvals.
- Reading or leaking secrets.
- Modifying core code without a checkpoint.
- Modifying judges to pass itself.
- Modifying risk map to lower its own risk level.

## 13. Recommended First Version Scope

First version should include:

```text
candidate archive
lineage events
rule judge
diff judge
test judge summary
cost summary
basic utility score
```

Defer:

```text
full clade search
long-running autonomous evolution
cross-provider judge by default
automatic high-risk apply
complex benchmark leaderboard
```

## 14. Acceptance Criteria

First version acceptance:

- Every repair/evolve candidate has an archive record.
- Every candidate links to parent/proposal/run.
- Success, failure, and rejection are all preserved.
- Judge output is structured.
- Deterministic gate failures cannot be overridden by model judges.
- Selection score is explainable.
- Lineage can show basic parent-child relationships.

## 15. Final Judgment

The Archive / Lineage / Judge / Selection theory is complete enough for first-version data structures and gates.

HGM/DGM-style long-running autonomous lineage search should wait until repair, knowledge, failure memory, and basic archive have real data.

---

<!-- Source: docs/design/octocode-self-evolution-architecture.en.md -->
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

---

<!-- Source: docs/roadmap/octocode-self-evolution-roadmap.en.md -->
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

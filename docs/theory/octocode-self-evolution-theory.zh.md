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

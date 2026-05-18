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

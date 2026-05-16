# DS vs 6 CLI 对照：逐模块完整矩阵（更新版）

更新时间：2026-05-15

来源：官方文档 / 公网仓库 / 官方 help（本地快照见仓库历史与子任务产物）。

## 0) 快速结论

1. DS 已具备：Rust 核心能力、TUI、会话、策略与批准框架、MCP stdio 基础、Subagent/Swarm、任务与事件持久化的雏形。
2. DS 最大短板：`命令发现与可达性` 和 `第一屏可操作入口`。
3. 从 UI 可用性角度，首屏应优先解决：
   - “输入区总是可见 + 可提示上下文动作”
   - “状态行字段按优先级折叠”
   - “欢迎页把命令面板入口显式化”
4. 从功能可用性角度，首阶段优先保留已有架构、补齐：
   - `/` 命令面板与命令发现
   - `Ctrl-R` / 历史回放可搜索
   - `@` 补全的文件路径入口（含路径提示）
   - `ds features/agents/mcp/help/model` 可发现的命令面。

## 1) 模块对照（主模块 vs 子模块）

### 1.1 CLI 命令与会话主干

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 主任务 |
|---|---|---|---|---|---|---|---|---|
| 主命令入口 | `codex`（exec/start/resume） | `gemini` + `-p`/`--output-format` | `claude` + `-p`/`--continue` | `kimi` + `--plan`/`--print` 等 | `droid` + `exec` + session flags | `qwen` + `qwen -p` | `ds` + `run/agent/mission/resume/chat` | 保持并发/会话语义一致，补齐标准入口一致性
| 无交互/机器可读 | JSON/stream-json、resume logs | JSON/headless 与 tool events | headless 与 JSON-ish 记录 | print/trace + resume | 非交互 exec 会话 | JSON/脚本化能力 | CLI 已有，但需要统一事件字段 | 标准化 machine output schema
| 会话恢复 | `--resume` | `/chat resume` 与历史 | `--continue`/`--resume` | `/chat resume` | `exec -s`/会话 id | `/chat resume` | `ds resume` 与 mission 复用 | 对齐恢复体验并明确命令层语义 |

### 1.2 TUI + 输入层

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 输入前缀系统 | `/` slash、`!` shell、文件/注释上下文 | `/` 命令、`!` shell | `/` commands，`@` + 文件路径语义 | `/` + shell 与恢复 | `exec` 风格命令交互 | slash 与 tool 命令 | 输入可用，但命令发现/补全较弱 | 上线命令建议面和 `Ctrl-R` 历史检索 |
| 欢迎页 | 初始化引导简洁 | 入口指引清晰 | 多上下文提示 | minimal / shell 起步 | 简化会话启动 | assistant 首屏指令清晰 | 欢迎页信息重排较新，但命令入口仍弱 | 在首屏放置可执行命令入口卡 |
| 上下文状态行 | 状态+模式+工具提示 | top/bottom status hints | status/思考提示 | compact 提示 | 轻量状态显示 | prompt/命令提示 | 已有状态line，但信息过密 | 折叠策略重排（core优先） |

### 1.3 命令发现与帮助

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 全量命令列表 | `/help` + 内置 docs | `--help` + `/help`/指令引用 | `/help` + 文档索引 | commands list（含扩展） | CLI reference（固定命令） | `/commands` + extensions | `ds features` 新增过，但未完全打通 TUI | 命令可达性低，需统一 command palette |
| 自定义命令加载 | command pack | custom commands/toml | `/agents` 与 skill/memory 文件 | agent / command hooks | 有限公开 | `/commands` + 自定义 | `.deepseek-code/commands` 还未作为一等入口 | 统一 command 面 + loader |
| 命令上下文开销 | 统一 help 与当前上下文注入 | 命令上下文提示 | 分层 /agent /tasks /model | 模式化命令入口 | simple 执行命令 | 命令+参数约定 | 命令与输入提示未统一 | 命令提示与执行反馈结构化化 |

### 1.4 权限 / 安全 / 审批

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 核心模式 | default/auto/full-auto/plan | plan/shell/approvals | default/plan/auto/bypass | plan/default/auto-edit/yolo | 公开细节有限 | 安全模式与沙箱约束 | ds 已有 policy+审批 + mode 但命名与展示需统一 | 标准化权限状态机可见性 |
| 工具策略 | allow/deny + sandbox | tool-level + plan profile | permission rules + tools filter | command/沙箱开关 | 企业化 review 路径 | 规则与 sandbox | policy.rs 已覆盖大部分 | UI 展示审批来源与可复用上下文 |
| Hooks | hooks 分层 | hook + project trust | 多 hook 类型 | 关键步骤 hooks | 少量流程钩子 | hooks/事件 | schema 存在，runtime 不完整 | 逐步接入 `PreToolUse/PostToolUse/UserPromptSubmit` |

### 1.5 Agents / Team / 任务编排

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| Agent 定义 | 内建 + 自定义 profile | agents/skills/extensions | agents 文件 + tool/mode | agents 概念逐步公开 | 工作流子任务 | agents/skills 配置 | `.deepseek-code/agents/*.md` + executor | 显式 agent 管控面薄 |
| 并发执行 | 多任务/队列 | 自动化 agent plan | 并行任务 | loop + retry | session/后台流程 | subagent-like flows | Supervisor/Swarm/MessageBus 已有 | UI 需显示状态与切换 |
| 任务队列/复用 | run history | resume/chat-history | tasks list | run session | bg process | session/retry | mission/event 已开始 | 统一任务目录与回放面 |

### 1.6 工具、MCP 与扩展

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| MCP/工具总线 | stdio + http + 配置 | stdio/http/sse + auth | tools + hooks + memory | MCP docs 与集成 | tools/集成 | MCP 命令与 auth | stdio 客户端已在 | 补 HTTP/SSE、工具可视化来源 |
| 技能/扩展 | 技能+插件+主题 | skills/extensions | SKILL.md + tool packs | 命令包/扩展入口 | enterprise workflow | command packs | 实现仍分散 | 统一 extensions 打包模型 |
| 日志与可追溯 | tool call 日志 + tool events | tool events + command events | 审计型记录 | 日志 trace | session log | session logs | DS 有 event/mission，UI 关联待增强 | UI 直接显示来源和失败路径 |

### 1.7 配置 / 可观察性 / 可用性

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 配置层级 | system/user/project | env + config + 受信项目 | 多层 + 本地覆盖 | project + user + trust | 官方文档可配置项少 | `.qwen` + settings | storage::Config 已有优先级 | 增加 `config explain/doctor` |
| 可见性/状态 | status, approval summary | settings + 权限状态 | status/permission + context | 命令视图 | process status | status 命令 | statusline/info 已有但密度高 | 折叠信息、保留关键字段 |
| 历史与日志 | session export | resume/share | plan/task traces | chat history | run history | chat logs | transcript/session 存在 | 统一命令和截图式回放 |

## 2) DS 侧立即执行建议（按优先级）

### P0（本轮先做）
1. 输入可达性：在空输入显示 `/help /agents @path !cmd` 提示（已落地）。
2. 布局自适应：超小终端去掉 model_hint/footer 冗余（已落地）。
3. 欢迎页：新增快速入口（`/help`, `/agents`, `/model`, `@path`, `!command`）（已落地）。
4. 状态行：将 status/permission/context 拉到固定优先级、隐藏不必要字段（部分落地，需联调）。
5. 状态线 `context_limit` 使用真实上下文上限（参数化）。

### P1（下一轮）
1. `/` 命令面板统一到 plan_tracker：支持分页、过滤、分组、命令说明。
2. `ds features` 与 `ds agent/mcp` 命令文案映射到欢迎页与 statusline。
3. `@` 与 `!` 的补全入口加入 suggestion panel（当前 suggestion 面已有数据）。
4. `Ctrl-R` 历史搜索与近似匹配。

### P2（后续）
1. `ds mcp add/list/remove` 命令族与 trust 状态。
2. Hooks runner（PreToolUse/PostToolUse/UserPromptSubmit）。
3. Stream JSON output 标准 schema。
4. Mission/replay 可视化页。

## 3) 开放源代码 vs 闭源对照标签

- 开源可直接复用：Codex CLI（命令架构/patch 流）、Gemini CLI（命令/plan/mcp）、Qwen CLI（命令风格与接口组织）、Kimi CLI（plan/shell/restore 风格）。
- 闭源可借鉴交互策略：Claude Code（权限模型/上下文管理/agent治理思路）。
- Droid CLI 公开文档较少，主要复用 `exec session + 执行流程` 的简洁风格。

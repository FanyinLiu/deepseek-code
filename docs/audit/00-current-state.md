# octocode 现状自审（Phase 1 / Task #1）

> 范围：Cargo workspace 根、`src/` 的全部 TUI 与关键后端模块、`docs/` 已有的 audit 文档。
> 用途：作为后续 6 份竞品调研报告的对照基线。**结论只代表 2026-05-13 当时仓库 HEAD 的实现**——后续任何 commit 都可能让本文档过期。
> 状态：**第一版草稿**。UI 子系统已读完（除 `tui/app.rs` 和 `tui/transcript_view.rs` 因体积过大需分块）。后端 `agent/*`、`policy/*`、`tools/*`、`mcp/*` 待补。

---

## 0. TL;DR：本项目的 14 个最显著的"现状特征"

1. **Rust + ratatui 0.30 + crossterm 0.29** 栈，二进制目标三个（`octocode` / `octocode` / `octocode`）。
2. **DeepSeek 原生**：内置 client (`src/deepseek/`) 支持 Pro/Flash/LegacyChat/LegacyReasoner、thinking mode、FIM、cache、json mode、migration、stream。
3. **CLI 命令面已经很宽**：`chat / ask / search / plan / run / resume / export / assess / review / features / agent / mission / task / tui / preview-tui`，外加 `doctor / login`。所有命令都接受全局 `-C / --project-root`。
4. **TUI 主布局是经典纵向 6 行栈**：`status_bar(1) / spacer(1) / 内容(min 5) / model_hint(1) / divider(1) / input(1-5h) / footer(2)`——没有侧栏，没有分屏；多面板都是把"内容"区临时换成 plan / subagent / diff / settings 视图。
5. **三套主题**：Light（Droid-tan 配色）/ Dark（暖琥珀 accent）/ HighContrast，靠 `COLORFGBG` + macOS `AppleInterfaceStyle` 自动检测，可 `--theme` 覆盖。
6. **TUI 默认走"安静 Droid 风"**：Light 主题用米色 canvas（235,226,196）+ 橙红 accent（224,82,0），属于明显的 Droid CLI 风格借鉴。
7. **状态栏（statusline）有 8 个彩色 chip**：`octocode / mode / web / ↑in / ↓out / agent / ¥cost / cache / tools / permissions / context%`——但**这些 chip 的配色 hard-code 在 `statusline.rs` 内**，**不走 `theme::palette()`**，是显著的主题断点。
8. **状态栏底色用 CNY ¥** 显示成本——明确是为中国用户优化，但对国际用户来说是品牌信号。
9. **审批 UX 已是 inline 模式**（`render_approval_inline`），按 `[a]` 一次 / `[s]` 本轮 / `[d]` 拒绝；中英文按内容自动切换；7 档风险（SafeRead → Blocked）有不同色。还保留了 `render_approval_popup` 兼容老的 fullscreen 模式。
10. **斜杠命令的弹窗已经存在**（`render_slash_command_panel`），但实际 input.rs 里**没有触发它**——`/` 仅作为字符录入，没有触发补全菜单。这是 P0 缺口之一。
11. **Plan tracker 已经支持**：`plan N/M`、进度条 `━━━●───`、step kind 自动识别（read/search/edit/run/verify/agent/task）、duration 标签、warning 行、自动滚动。
12. **Subagent cards** 已支持多 agent 卡片化：图标按类型区（◈ explorer / ◇ reviewer / ▷ planner / ▶ test-runner / ● 通用）、显示 R/W 文件计数、token、duration、最近 3 行输出。
13. **Diff viewer 是逐文件 list**：每个文件有 ○/✓/✗ 状态 + 路径 + stats + diff body（带 syntect 高亮），可选 selected/scroll，但**底色 hard-code `BG_DEEP`**，不跟随主题。
14. **Decision panel**（`render_options_panel`）支持三种 kind：PlanAction / Clarification / Conflict，按 kind 切 header/color/copy；自带中英文，1-9/a-z 快捷键。

---

## 1. 包结构 / 二进制 / 入口

### 1.1 Cargo

```
[package]
name = "octocode"
version = "0.1.0"
edition = "2021"
default-run = "octocode"
```

依赖关键栈：
- TUI：`ratatui = "0.30"`、`crossterm = "0.29"`（带 `event-stream`）
- HTTP：`reqwest = "0.12"`（rustls-tls + stream + json）
- Async：`tokio` 全套（macros / rt-multi-thread / sync / time / fs / process / signal）
- 文本：`syntect = "5"`（默认 syntaxes + themes + regex-fancy）、`similar = "2"`、`unicode-normalization = "0.1"`
- 配置：`toml = "0.8"`、`serde / serde_json`、`dirs = "6"`、`keyring = "3"`、`fs2 = "0.4"`
- 文件/搜索：`walkdir`、`ignore`、`glob`
- 日志/追踪：`tracing` + `tracing-subscriber`（带 env-filter 和 json）
- CLI parsing：`clap = "4"`（derive）

发布 profile：`opt-level=3 + lto=true + strip=true`。

### 1.2 三个二进制

| 名字 | 路径 | 当前用途 |
|---|---|---|
| `octocode` | `src/main.rs` | 主入口，默认无参运行直接进 TUI |
| `octocode` | `src/bin/octocode.rs` | 短名快捷入口（"用户面向的干净命令"——见 `docs/openai_codex_deepseek_tui_comparison.md`） |
| `octocode` | `src/bin/octocode.rs` | 更短的 alias |

**问题**：三个二进制名带来品牌分裂。Codex / Gemini / Claude Code 都是一个二进制 + 别名（或者直接一个）。

### 1.3 CLI 子命令面（`src/cli_entry.rs`）

```
octocode [-C PATH]
    doctor
    login [--api-key KEY]
    chat [PROMPT] [-t/--thinking] [-m/--model pro|flash] [--session ID]
    ask QUESTION
    search QUERY [--code-only] [-l/--limit N]
    plan TASK
    run TASK [-t/--thinking]
    resume [SESSION_PREFIX]
    export [SESSION_ID] [-f markdown|json|text]
    assess TASK
    review [-p PARALLEL] [--max-turns N] [-o OUTPUT]
    features matrix | status | recommend TASK     [--json]
    agent list | show NAME | run NAME TASK | create NAME --template X | validate
    mission new TASK [--dry-run] | status | inspect | replay | list
    task list | add KIND PROMPT | pause | resume | run | logs | rm
    tui [-t] [-m] [--session]
    preview-tui --width N --height N --api {missing|ready} --scenario {welcome|workbench} --theme {light|dark|high-contrast}  (hidden)
```

`agent create` 模板已枚举：`Explorer / Reviewer / Auditor / Tester / Planner / Writer`。

**入口分支**：`cli.command.is_none()` && stdin/stdout 都是 TTY 时直接 `tui::run_tui(...)` 起 TUI；否则进 `cli::welcome(...)`。

---

## 2. UI 子系统逐文件结论

> 顺序按视觉重要性：主题 → 布局 → 欢迎 → 输入 → 状态栏（顶/底）→ 对话区 → 决策弹窗 → 审批 → diff → 子 agent → 文件树 → 设置。

### 2.1 主题系统 `src/tui/theme.rs`

**结构**：
- `ThemeMode`：Light / Dark / HighContrast，可 `toggled()` 三态轮换。
- `ThemePalette`：15 个 color slot（canvas / surface / surface_alt / input / text / secondary / dim / muted / divider / accent / success / warning / danger / info / inverse_text）。
- 三个常量 palette：`LIGHT_PALETTE`（Droid 米色 + 橙红 accent）/ `DARK_PALETTE`（暖琥珀 accent）/ `HIGH_CONTRAST_PALETTE`（纯黑底 + 鲜亮 accent）。
- 全局 `ACTIVE_THEME: AtomicU8`（测试模式下是 thread_local）。
- 老式常量层（`BG_DEEP / BG_BASE / BG_CARD / BG_CARD_HOVER / FG_PRIMARY / FG_SECONDARY / ACCENT_AMBER...`）保留作为遗留入口。

**亮点**：
- 自动检测明暗：先看 `COLORFGBG` env，再 macOS `defaults read -g AppleInterfaceStyle`，兜底 Dark。
- `Color::Reset` 用作 canvas（不强制覆盖用户终端底色，对终端原生主题友好——这点 Codex CLI 也是这么做的）。

**问题**：
- 还存在 deprecated alias（`BG_CARD_ALT`）和大量 module-level color 常量没接进 palette，意味着部分组件直接引用 `theme::BG_DEEP`、`theme::ACCENT_AMBER` 等老常量，跳过 palette → **theme 切换时这些组件不会变色**。明显的违例是 `diff_viewer.rs` 和 `statusline.rs`。
- `Style helper` 一大堆（`style_primary / style_secondary / style_dim / style_user / style_assistant / style_input / style_status_ok / ...`），但没有"role 表"系统化，调用方靠记忆。
- `USER_BG / ASSISTANT_BG` 都等于 `BG_DEEP`——意味着用户/助手消息**目前没视觉区分**（仅靠前面的角色标签）。

### 2.2 主布局 `src/tui/layout.rs`

只有 73 行，核心是 `app_layout(area, input_height) -> (status, content, model_hint, divider, input, footer)`：

```text
┌─────────────────────────────────────┐
│ status_bar (1 row, top)             │ ← 顶部活动指示器（spinner + token）
├─────────────────────────────────────┤
│ spacer (1 row)                      │
├─────────────────────────────────────┤
│ content (min 5)                     │ ← 欢迎页 / transcript / plan / diff / subagent / settings
├─────────────────────────────────────┤
│ model_hint (1 row)                  │ ← 右对齐 "DeepSeek V4 Pro (auto)"
├─────────────────────────────────────┤
│ divider (1 row)                     │ ← 一条 ─
├─────────────────────────────────────┤
│ input (1-5 rows)                    │ ← 多行输入，按内容增高
├─────────────────────────────────────┤
│ compact footer (2 rows)             │ ← statusline (chips)
└─────────────────────────────────────┘
```

输入区高度 `clamp(1, 5)`——超过 5 行强制截断。

辅助函数：`search_layout()` 25/45/30 三栏（标记 `#[allow(dead_code)]`——未实际启用），`sidebar_layout()` 固定 sidebar 宽 + 主区，`card_inner()` 给 card 加 2/1 内边距。

**问题**：
- **没有真正的分屏 / sidebar**——`sidebar_layout()` 看着像准备的接口，但实际用例只有 transcript 单栏。
- 输入区上限 5 行偏紧，长 prompt（粘贴 100 行 stack trace）会被截。
- 整个布局没有 modal/dialog 层，所有"弹窗"实际上是替换 content 区。
- footer 是 2 行 statusline——大部分终端用户看到的"底栏"占了双行，相对其他 CLI 偏厚。

### 2.3 欢迎页 `src/tui/welcome.rs`（349 行 + 测试）

**响应式布局**：
- `< 54×14` → `render_compact_welcome`（标题 + 数据简略行 + 1-3/enter 提示）
- `< 96×22` → `render_compact_welcome`（鲸鱼品牌块 + compact 操作提示）
- `≥ 96×22` → `render_split_welcome`（左 54% identity / 中 1 列分隔条 / 右 46% actions）

**WelcomeDashboardData**：
- `workspace_name / workspace_path / model / thinking / api_key_status / config_status / cache_status`
- `recent_sessions: Vec<RecentSessionItem>`（取最近 3 个）
- `skills`（14 个内置技能：read_file / edit_file / write_file / search_code / run_command / git_workflow / web_search / github_pr / semantic_search / fetch_url / image_input / lsp / subagent / mcp）
- `mcp_servers`：从 `Config::load(...).mcp.servers` 读，状态分 Connected / Failed / NotConfigured
- `agents_md: AgentsMdInfo`（loaded / rule_count / summary）
- `detected_language`：扫项目根的特征文件（Cargo.toml → Rust，package.json → JS/Node，...）

**视觉元素**：
- 中心 `ascii_art::WELCOME_WHALE` / `WELCOME_WHALE_COMPACT`（Octocode 鲸鱼品牌块）
- 提示行 `Tip: Use /init to teach Octocode this workspace`
- 快捷键行：`shift+tab 切 mode · ctrl+n 切 model / ctrl+l autonomy · tab thinking`
- Capability 行：`skills (N) + · MCP (N) ? · AGENTS.md +/x`
- API 缺时显示 3 步引导（粘贴 / Enter / 开始）
- 有钥匙时提示 `1-3 starters · / commands · @ files · ! shell · enter send`
- starters 是硬编码的 3 个 prompt：`Inspect workspace for next fix` / `Find TUI entry points` / `Run checks and summarize`

**问题**：
- 3 个 starter prompt 完全 hardcode，没有按项目语言/最近活动智能化——和 Claude Code / Codex / Gemini 都有差距。
- "capability line" 用 `+ / x / ?` 符号表达，扫起来不直观；竞品多用 ✓ / · / ! 或者圆点。
- 欢迎页**没有显示"安全模式"或"是否在 git 仓库"**——这是 Codex / Claude Code 都会在首屏展示的关键安全信号。

### 2.4 ASCII 艺术 `src/tui/ascii_art.rs`

当前保留 DeepSeek 鲸鱼品牌块：
- `WELCOME_WHALE`：常规欢迎页使用
- `WELCOME_WHALE_COMPACT`：窄屏欢迎页使用
- `LOGO_TINY` / `WHALE_TINY`：状态行和极窄界面使用

**问题**：
- 只有 ASCII 单色版本；没有 light/dark 不同笔画粗细备选。

### 2.5 输入框 `src/tui/input.rs`

**核心**：`render_input(f, area, input_text, _pending_options)`——
- 无边框、无背景填充（用 canvas 底色），第 1 行带 `›` chevron（accent 色），后续行用 2 空格前缀
- 多行直接靠 `\n` split
- 真实光标位置由 caller 通过 `terminal_cursor_position(...)` 拿到再 `set_cursor`——不渲染字符光标块，靠终端原生
- 支持 CJK 双倍宽度（`char_display_width`）+ 密钥模式（• 遮罩）

**API key 输入**：`render_api_key_input(f, area, text)`——空时显示 "paste API key..." 占位，输入时全部 mask。

**问题**：
- 完全没有 inline 补全 UI——这里只渲染文字，没有 `/` 弹窗、`@` 文件提示、history dropdown 等任何 picker。
- `_pending_options` 是占位参数（前缀下划线），说明设计时考虑过 inline pending UI，但实现没接。
- 没有 syntax highlighting 输入（粘贴代码也是纯文本）。
- 没有展示 paste 检测（Codex / Claude Code 都做了 "粘贴 N 行" 的折叠卡片）。

### 2.6 顶部状态栏 `src/tui/status_bar.rs`

`AppMode`: Chat / Plan / Run / Review（4 模式，各自一种 accent color：橙红 / 蓝 / 绿 / 紫）。

`render_status_bar` 只在有 `activity` 时画一行，**idle 渲染空**：
```
⠴ Fix input colors... (6s · ↓ 578 tokens · thought for 2s)
```

**Spinner**：Braille 10 帧（`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`），按 `elapsed_ms / 80` 取帧。

**State 文案**：
- 有 `thought_seconds` & `tokens` → `thought for {N}s`
- `elapsed >= 90s` → `almost done`
- `elapsed >= 35s` → `still thinking`
- 其他 → `thinking`

Token label 单方向：output 优先（`↓ 578 tokens`），否则 input（`↑ 41 tokens`），否则 agent（`agent 16.2k tokens`）。

**亮点**：
- 单行不抢屏，"thinking → still thinking → almost done"渐进文案是好设计——类似 Claude Code 的 "thinking" 长动画。
- CJK 任务标题用原文（"修复输入颜色…"），spinner 用 ASCII。

**问题**：
- Mode 颜色与 statusline 的 mode chip 颜色不一致（这里用 theme 模块的 `DROID_ACCENT/ACCENT_BLUE/...`，statusline 用自己的硬编码 `mode_bg`）。
- 没有"按 Esc 中断"的明显提示。
- 没有 reasoning effort / streaming buffer / tool 调用等更细分的活动子状态。

### 2.7 底部状态栏 `src/tui/statusline.rs`

8-10 个彩色 chip，按宽度自动隐藏（narrow < 88、compact < 112）：
```
 octocode   chat   web:on   ↑ 742 tokens   ↓ 131 tokens   agent 16.2k tokens   ¥0.003   cache 80%   tools ✓   ask   126.3k/1M (12.6%)
```

每个 chip 有自己的 RGB 底色：
| Chip | bg | 含义 |
|---|---|---|
| `octocode` | `rgb(36,38,42)` | 项目品牌色 |
| mode | `rgb(87,142,214)` 蓝 | chat/plan/run/review |
| web | `rgb(118,184,124)` 绿 | web search on/off |
| ↑in | `rgb(255,184,77)` 橙 | input tokens |
| ↓out | `rgb(102,204,204)` 青 | output tokens |
| agent | `rgb(158,206,106)` 黄绿 | subagent token |
| ¥cost | `rgb(188,139,216)` 紫 | RMB cost |
| cache | `rgb(144,184,104)` 黄绿 | cache hit% |
| tools | `rgb(111,191,113)` 绿 | tools 状态 |
| permissions | `rgb(198,72,82)` 红 | ask/bypass |
| context | `rgb(244,206,22)` 黄 | tokens / 1M (%) |

`CONTEXT_LIMIT_TOKENS = 1_000_000` 写死。

**问题**：
- **配色完全不走主题**——切到 Light 主题这条彩色 chip 行依然是 dark 配色，视觉断裂。
- 11 个 chip 即便 narrow 也保留好几个，**信息密度太高**，对比 Claude Code 的 statusline（context % + 模型 + dir）极简。
- `¥{:.3}` 写死 CNY 货币符号 — DeepSeek 用户场景合适，但国际化需要可配置。
- `context_limit_label` 硬编码 `"1M"` —— 不同 model 有不同 ctx window（V4 Pro vs Flash），这里没适配。
- compact 模式下 cache 直接整列消失，但 cache hit rate 是 DeepSeek 的关键卖点之一，逻辑不合理。

### 2.8 model hint `src/tui/model_hint.rs`

中间那行右对齐显示 `DeepSeek V4 Pro (auto)`。

`render_composer_hint` 是变体：左侧塞 spinner+task activity，右侧 model hint，中间空白，按宽度截断 activity 部分。

——和 status_bar 信息基本重复，这是一个**冗余风险**。

### 2.9 对话区 `src/tui/transcript_view.rs`（~1200 行，已读 1-1200/1200+，剩余主要是 inline subagent/diff helper 和测试）

**核心数据**：`TranscriptProps` 14 字段——messages / pending_user_message / scroll_offset / plan_summary / plan_steps / plan_current_step / plan_total_steps / plan_warnings / subagents / global_elapsed_ms / diffs / selected_diff / is_streaming / stream_buffer / reasoning_buffer / show_reasoning。

**整体哲学**："Single continuous terminal transcript. No role headers, no extra blank lines, content speaks for itself."——明显的 Claude Code 致敬。

**渲染顺序**（render_transcript）：
1. 历史消息（按 visibility 过滤 AuditOnly）
2. pending_user_message（用户已输入但还没发送的预览）
3. is_streaming → 当前 stream_buffer
4. show_reasoning → reasoning_buffer 用 `│ ` 灰边显示
5. inline plan（如果 plan_steps 非空）
6. inline subagents
7. inline diffs

**消息渲染**（render_message）：
- User：走 `render_user_message`→`user_text_lines`，**每行加 `▸ ` 前缀**，整行用 `user_bar_bg/fg` 深底亮字反相显示（dark 下 surface_alt 底，light 下 (42,42,42) 暗底 + 米色字）
- Assistant：`●` accent prefix + palette.text 色，调用 `render_assistant_content`
- System：`!` dim prefix
- Tool：`$` accent prefix；如果有 `tool_results`，每个 result 渲染为 `ToolCallView`（Done/Failed），调 `view_blocks::render_tool_lines`
- 普通 assistant 消息后的 `tool_calls` 渲染为 `Running` 状态的 ToolCallView，但**走 `render_claude_tool_lines`**（Claude Code 风格特殊路径）

**render_claude_tool_lines — 这是显著的 Claude Code 风格代码**：
| Tool | 标题文案 |
|---|---|
| `run_command` | "Running 1 shell command..." |
| `read_file` | "Reading 1 file..." |
| `list_dir` | "Listing 1 directory..." |
| `search_files / search_code / semantic_search` | "Searching workspace..." |
| `fetch_url / web_search` | "Fetching..." |

格式：
```
● Running 1 shell command...
  └ cargo test
```

**markdown 渲染层**（render_assistant_content）：
- 用 `syntax_highlight::parse_markdown` 切块
- Text 块：逐行处理，每行可能是表格起始、横线规则（`---/***/___`）、"Brewed for N" 特殊行、普通行（走 `inline_spans`）
- Heading：L1 accent、L2 warning、L3+ secondary，全 BOLD
- BlockQuote：`│ ` dim 前缀 + secondary 文字
- CodeBlock：缩进 2 格，调 syntect 高亮，**不显示 language 标签**（标注 "code blocks stay compact in the transcript"）

**inline 富文本解析**（inline_spans + next_inline_token）：识别——
- `**bold**` → BOLD
- `` `code` `` → warning 色 + BOLD
- URL（http://、https://、file://、chrome://、vscode://、app://、mcp:// 共 7 个 scheme）→ info 色 + UNDERLINED
- `/command_name` → info 色 + BOLD
- `--flag` → warning 色
- 关键标识符（TaskCreate / TodoWrite / TaskUpdate / addBlockedBy / status / in_progress / completed / ...）→ info 色 + BOLD
- 状态词（completed / done / ✓）→ success + BOLD
- 状态词（in_progress / running）→ accent + BOLD
- 状态词（blocked / pending）→ dim

**markdown 表格**——双模式：
- ≤4 列 + 总宽 ≤ max_width → 经典 box drawing 表格（┌┬┐ ├┼┤ └┴┘），表头加粗，单元格颜色按 success/running/blocked 状态着色
- > 4 列或超宽 → "card mode"：标题 "Table (N rows)" + 每行 `• id  title` + 子行 `  └ label: value`（适合窄屏的退化）

**Inline Plan**（render_inline_plan）：
- 头部 `  N tasks (M done, K open)` 或 swarm 模式 `  N agents`
- swarm 检测：所有 step.description 都以 "agent " 开头时认为是 swarm plan
- 中文判断：summary/steps/warnings 任意含 CJK → 用中文版（`N 项任务（M 完成，K 未完成）`、`… 前面还有 N 项任务` / `… 后面还有 N 项任务`）
- swarm summary 清洗："蜂群计划：" / "蜂群任务：" / "Swarm plan:" / "Swarm task:" 等前缀剥离
- 列表：左边 `  │ ` 灰边竖线 + status marker（`○ ● ✓ ✗` 之一）+ 编号 `12. ` + 标题 + duration
- 标题清洗：剥离 `Read \``、`Search \``、`Edit \``、`Run \``、中文版本 `读取 \``、`搜索 \``、`修改 \``、`运行 \``、`Verify — `、`验证 - `、leading 数字+点
- 可见范围：6 行窗口，focus 在 running step；超出时用 `… N earlier tasks` / `… N more tasks` 折叠

**应隐藏的 transcript 行**（should_hide_transcript_line）：
- `[Self-verification]` 系列
- `No verification available for this project type`
- `◇ running tool ` / `◆ done tool ` / `◈ running tool ` / `◈ running  tool `
- `intent ` 开头
- `detail --- ` / `detail todo.md` 开头

这是**输出净化层**——和 `defense/output.rs` 配合，避免内部协议噪音泄漏到 transcript。

**亮点**：
- markdown table 双模式（box / card）是非常贴心的窄屏适配
- inline token 高亮系统识别 7 种 URL scheme + 命令 + flag + 关键标识符——比大多数 CLI 的 markdown 渲染都细
- 中英文自动切换覆盖 inline plan 全部 copy
- "Claude Code 风格"的 tool running 卡片显示了明确的视觉对标方向

**问题**：
- inline_spans 不支持 `*italic*` / `__underline__` / `~~strike~~` / markdown 链接 `[txt](url)` / image
- table card mode 把第二列视为 "title" 是 hardcoded 假设——不一定对
- 列表项（`- item` / `* item` / `1. item`）**完全不识别**——会作为 plain text 行渲染（这是大短板）
- should_hide_transcript_line 用字符串前缀匹配，fragile
- "Brewed for N" 看起来是 GitHub Copilot 或类似产品的标志——为什么 octocode 会输出这个？需查
- `render_inline_subagents` / `render_inline_diffs` / `aggregate_plan_status` 未读到，估计是中规中矩的辅助函数

### 2.10 决策面板 `src/tui/plan_tracker.rs`（包含三个 panel）

实际包含 4 个公共 render 函数：

**1) `render_plan_tracker_with_warnings`**:
- 头部："plan 2/5  This is the plan summary"
- 可选 warning 行（⚠ 黄）
- 进度条：`━━━●───`（done / running / pending）
- 列表：每条 step 是 `▶ ● kind     status   description · took 12s`
- 状态机：Pending(○ 灰) / Running(● 绿 加粗) / Done(● 灰) / Failed(● 红)
- 自动滚动到当前 running step

**2) `render_options_panel`** —— Y/N/多选决策面板：
- 三种 `DecisionKind`：PlanAction（accent）/ Clarification（warning）/ Conflict（danger）
- 每种 kind 有专属 header / prefix / row_tag / hint copy（中英文双版本）
- 行格式：`▶ 1. [tag] description`（▶ 选中标记，1-9/a-z 快捷键，[执行]/[选]/[处理] 标签）
- 底部 hint：`↑↓ 移动  Enter 执行所选  Esc 取消  1-9 快捷键`

**3) `render_slash_command_panel`** —— `/` 命令补全菜单：
- 头部：`Commands  ↑↓ choose  Enter run exact / complete partial  Tab complete`
- 行格式：`▸ name             description`（动态计算 name 列宽 8-18）
- 注意：**这个 panel 存在但目前 input.rs 不触发它**——存在但未联动。

**4)（不在本文件，注意 `plan_tracker.rs` 名字与功能不匹配）**：把 options panel + slash command panel 都放在 plan_tracker 里，命名糟糕，应该拆分。

**亮点**：
- 中文用户自动切中文 copy，这一点比 Codex/Gemini 都强。
- 三种 DecisionKind 区分 severity——和审批 popup 的 risk_level 是独立两套，可考虑统一。

**问题**：
- step_kind 是字符串前缀匹配（"Read \`", "Edit \`", "agent ", "Search \`"...），fragile，重命名 plan executor 输出格式会断。
- progress_bar 没有 percentage 数字，长 plan 时不直观。
- 没有"暂停 / 单步 / 跳过"控制——Plan 是只读的。

### 2.11 审批 `src/tui/approval_popup.rs`

**inline 模式**（默认）：
```
approve tool call · CommandExecution
 tool      run_command
 intent    cargo test
 command   cargo test
 cwd       project root
 path      src/main.rs
 [a] approve once  [s] approve session  [d] deny
```

7 档 `RiskLevel`：SafeRead（绿）/ SensitiveRead（黄）/ WriteProject（黄）/ GitMutation（accent）/ CommandExecution（红）/ NetworkAccess（红）/ Blocked（红）。

中英文按 `approval.title/description/details` 含 CJK 自动切。中文文案：审批工具调用 / 安全读取 / 敏感读取 / 写入项目 / Git 修改 / 执行命令 / 网络访问 / 已阻止 / 批准一次 / 本轮批准 / 拒绝。

`render_approval_popup` 是 legacy 居中悬浮版本（带 `Clear` 清屏），高 6-N 行，宽 80。

**亮点**：
- 把 details 按 `key: value` 模式自动拆 row 显示，非常清晰。
- inline-first，符合"copyable classic terminal mode"方向（`docs/architecture_map.md`明确的方向）。

**问题**：
- 只有 3 个动作（once/session/deny），没有 Codex 的 "always allow this command" 或 Claude Code 的 "add to permissions.allow"。
- "本轮批准" 持久化生命周期没在 UI 里说清。
- 没有"修改命令后再批准"的 inline 编辑能力（Codex 有）。

### 2.12 子 agent 卡片 `src/tui/subagent_cards.rs`

`SubagentCard` 字段：agent_id / agent_type / description / status (Running/Done/Failed) / start_time / last_update / recent_lines (VecDeque, max 3) / summary / duration_ms / files_read / files_written / token_usage / is_background。

`render_subagent_cards`：
- 头部：`agents 2/5`（running/总数）
- 每张卡走 `view_blocks::render_worker_lines`：
  ```
  ● running  agent ◈ code-explorer abc123
   task     Explore the tui module
   time     12.3s
   files    R3 W0
   tokens   1240
   summary  ...（如有）
  ```
- running 状态额外用 `╎` 边框列出最近 3 行 stdout
- 倒序展示（最新的在上），按区高度截断

Agent icon 表：`code-explorer ◈`、`code-reviewer ◇`、`planner ▷`、`test-runner ▶`、默认 `●`。

**亮点**：
- 卡片元数据格式（R/W 计数 + token + duration）信息密度合适。
- "最近 3 行" 是好取舍——既能看进展又不刷屏。

**问题**：
- 没有"展开 / 折叠 / 取消"控制（Claude Code Task tool 的 UI 有按钮）。
- background 用 `[bg]` 文字标记，不显眼。
- 没有 agent 之间的依赖/层级显示（一个 agent 派生出 sub-sub-agent 时看不出层级）。

### 2.13 view_blocks（共享 view 原语）`src/tui/view_blocks.rs`

提供 5 个 View 数据结构 + 渲染函数：
- `ViewStatus`：Queued / Running / Done / Failed / Denied / Cancelled（6 态）
- `DialogView` / `ToolCallView` / `TaskReturnView` / `WorkerReportView`
- 工具：`header_line / kv_line / divider_line / status_word`
- `compact_tool_line` —— 一行表达一次 tool call：`● done  tool write_file  changed todo.md`
- `render_task_lines / render_worker_lines`
- `classify_tool(name) -> "read"|"search"|"edit"|"run"|"git"|"fetch"|"mcp"|"tool"`
- `summarize_tool_arguments(name, json_str) -> short_label`（按 tool 名提取 path/command/query/url/message）
- `summarize_tool_result(result_str)`、`summarize_tool_detail(diff_line)`

**亮点**：
- 把 tool call 的渲染抽到了一个共享层，避免 transcript / subagent / plan 各自拼字符串。
- ViewStatus 的 6 态完整覆盖了"申请 → 通过 → 取消"全周期。

**问题**：
- DialogView 字段定义了但没看到使用方（在已读文件中），可能用在 transcript_view 里。
- 与 `plan_tracker::PlanStepStatus` 是两套独立状态机，存在冗余风险。

### 2.14 Diff viewer `src/tui/diff_viewer.rs`

`FileDiffItem`：path / diff（全文）/ stats（如 `+5 -2`）/ status（Pending ○ / Accepted ✓ / Rejected ✗）。

`render_diff_viewer(f, area, diffs, selected, diff_scroll)`：
- 多文件展开成一个长 list
- header 行：`○ src/foo.rs (+5 -2)`（selected 时 amber 加粗）
- diff body 按 `+`/`-`/`@@`/`---`/`+++`/`\` 上色，并尝试用 syntect 高亮 `+`/`-` 行的代码部分
- 滚动：`diff_scroll` 偏移；默认 anchor 在底部（"显示最新")
- 内部 wrap = false

**问题**：
- **不走主题** —— 整个 widget 用 `style.bg(theme::BG_DEEP)` 写死，light 主题下视觉异常。
- 没有"接受 / 拒绝 / 跳过 / 编辑后接受" 的 inline 操作 affordance。
- 没有 side-by-side 模式（unified-only）。
- 没有 hunk 级别折叠（长 diff 不可隐藏）。

### 2.15 Syntax highlight `src/tui/syntax_highlight.rs`

- 用 `syntect 5` 的 `SyntaxSet::load_defaults_newlines` + `ThemeSet::load_defaults`
- 主题**硬编码** `"base16-ocean.dark"` —— 不分明暗。
- `highlight_code_block(code, language)` → `Vec<Vec<Span>>`（每行 Span 列表）
- `lang_from_path(path)` → 20+ 扩展 → token 映射
- 内置一个**简化版 markdown parser**：识别 ```fenced code、`# / ## / ###` heading、`> blockquote`、其他作为 plain Text

**问题**：
- syntect 默认 theme set 包含 6-8 个主题，但代码只用一个，无切换。
- markdown parser 极其简化——不识别表、列表（`-` `*` `1.`）、行内 code（\`...\`）、emphasis（\* \_）、链接、image。**这是对话区 markdown 渲染的根本短板**——后面 transcript_view 八成会因为这个 parser 的不足而出问题。
- syntect 启动需要 load 默认 syntaxes（不算便宜），但用 `OnceLock` cache 了。

### 2.16 文件树 `src/tui/file_tree.rs`（245 行）

`FileTreeNode` { path / name / is_dir / depth / is_expanded / children }；`FileTree` { root / nodes / selected / scroll_offset }。

实现：
- `WalkBuilder::new(dir).max_depth(Some(1)).hidden(false).git_ignore(true)`——**懒加载（只展示 1 层）**，**展示隐藏文件**，尊重 `.gitignore`
- `toggle_expand` 时才扫子目录
- `navigate_up/down`、`selected_path()`、`selected_is_dir()`
- 排序：文件夹优先，组内按 lowercase 名字
- 渲染：rounded border + " Files " amber 标题；`▾`/`▸` 展开图标；selected 用 `BG_CARD_HOVER` 反相

**问题**：
- 用 `theme::ACCENT_AMBER / BG_CARD / BG_CARD_HOVER` 老常量，**不走 palette**，light 主题视觉异常
- 展示隐藏文件（`hidden(false)`）但只 `git_ignore`——`.ignore` / `.codeagentignore` 等其他忽略规则全部不读
- 没有 fuzzy filter / 搜索框 / 跳转
- 没有 git status 染色（M/A/D 标记）
- 当前在 layout 里**没有实际启用**——`sidebar_layout()` 标记 dead_code，文件树渲染函数存在但主 app.rs 是否调用待确认

### 2.17 设置面板 `src/tui/settings_panel.rs`

```
Settings (read-only)

Model
  Provider              deepseek
  Active model          <session model>
  Default model         <config model.default>
  Heavy model           <config model.heavy>

Safety
  Autonomy level        <config policy.autonomy_level>
  Safe reads            <config policy.auto_approve_safe_read>
  Auto mode             <config policy.auto_mode>
  Write approval        <config policy.require_approval_for_write>
  Command approval      <config policy.require_approval_for_command>

Interface
  Language              <config ui.language>
  Theme                 <current theme>
  Motion                <config ui.motion>
  Renderer              <current renderer>

Agents
  Router                <config router.enabled>
  Subagents             <config subagent.enabled>
  Swarm                 <config subagent.swarm_enabled>
  MCP                   <config mcp.enabled>
  Hooks                 <hook count>
```

设置面板仍是只读快照，但现在只展示真实配置或当前会话状态；未实现的音效、statusline 形状、prompt precache 等占位项不再进入 UI。

### 2.18 主事件循环 `src/tui/app.rs`（76013 tokens / 估计 2500+ 行，未分块读完）

**未读完**——文件过大。下一轮分块读。预期内容：crossterm event loop、key 处理、模式切换、视图状态机、所有 TUI 交互 dispatch、preview snapshot 路径。

从 `cli_entry.rs` 暴露的 PreviewTui 命令可知，`app.rs` 内有 `render_preview_snapshot(root, missing_api, w, h, scenario, theme)` 公共函数 + `PreviewSnapshotScenario` 枚举（Welcome / Workbench）。

---

## 3. 已知的预期改造方向（来自 `docs/openai_codex_deepseek_tui_comparison.md` 2026-05-09）

之前已经有人列了 5 步推荐顺序，原文摘录：

> **Immediate Next Implementation Order**：
> 1. `/` command palette in TUI.
> 2. Ctrl-R history search.
> 3. `@` file mention completion.
> 4. `/agents` and `/tasks` command surfaces.
> 5. Skills loader.

以及 4 个优先级分层（P0 让 TUI dependable、P1 权限现代化、P2 子 agent 产品化、P3 生态对齐、P4 发行打磨）。

**这次大改造和上次的 alignment**：本次用户要求"从 UI 到功能、从大框架到小细节"全量重做，应在上次 P0-P4 路线图基础上做**深度横向扩展**——不仅完成上次 P0 5 项，还要把每项做到对标 6 个竞品后的"行业最佳"水平。

---

## 4. 现状 UI 的 "11 大看得见的痛点" （初步清单，调研后补全）

1. **欢迎页 ASCII wordmark 形状不像 "DeepSeek"**——需要重新设计，或加 narrow/wide 自适应。
2. **3 个 starter prompt 完全 hardcode**——应该按项目语言、最近 session、git 状态动态生成。
3. **底部 statusline 11 个彩色 chip 配色完全不走主题**——切到 Light 主题时配色断裂；信息密度过高。
4. **slash command panel 已实现但 input.rs 没触发**——`/` 没有补全菜单。
5. **没有 `@` 文件提及补全 UI**。
6. **没有 Ctrl-R 历史搜索 UI**。
7. **diff viewer 强制深底色**——不跟随主题。
8. **syntax_highlight 强制 base16-ocean.dark**——light 主题下代码块对比度奇怪。
9. **markdown parser 太简化**——不支持表、列表、行内 code、emphasis、链接——直接影响对话区 markdown 渲染质量。
10. **input.rs 完全没有 inline picker**——没有 paste 检测、IDE 输入感、自动建议、命令补全。
11. **status_bar 和 statusline 信息重复**——activity 在两处显示。

更多痛点等读完 `transcript_view.rs`、`app.rs`、`agent/*`、`commands/*` 后补全。

---

---

## 3. 后端架构（基于 module signature 扫描 + 关键文件细读）

### 3.1 `agent/*` 子系统 —— **存在 5 套并行的"多 agent 协同"抽象**（设计冗余！）

| 模块 | 行数估计 | 核心抽象 | 实际作用 |
|---|---|---|---|
| `agent/subagent/*` | 大 | `SubagentConfig / SubagentResult / SubagentTask / SubagentType` + executor + registry | **底层执行单元**：跑一个独立 agent，返回 `SubagentResult` |
| `agent/team.rs` | ~150 行 | `AgentRole / TeamPlan / TeamPlanDraft / TeamMilestone / TeamTask / TeamTaskMode / TeamRun / AgentRunState` | **目标"canonical flow"**（docs 明确）：`TeamPlan → TeamRun → TeamTask → AgentRun`，是稳定的高层抽象 |
| `agent/swarm.rs` | **1666+ 行** | `SwarmCoordinator / SwarmRun / SwarmPlan / SwarmPlanDraft / SwarmTask / SwarmAgentRole / SwarmTaskStatus / SwarmResult / SwarmPendingPatch / SwarmRunOptions / SwarmEvent / SwarmIntent / SwarmPlanQualityGate` | **最重的实现**：当前 swarm（多 agent 并行）的核心。含 7 种 prompt 模板（explorer/explorer_scope/reviewer/tester/worker/verifier/planner）、pending patch 冲突检测、validation gate、双语 |
| `agent/task_tool.rs` | 极小 | `TaskToolHandler` | **Claude Code 风格的 Task 工具适配** |
| `agent/lanes.rs` | ~110 行 | `classify_task(input) -> TaskClass` + `TaskClass` 枚举 | **不是多 agent 抽象**，是任务**分类器**：判断输入需要 chat / plan / agent / swarm 哪种 lane |

**问题**：
- **team.rs 与 swarm.rs 是同一个问题的两个解** —— team 是"想要的"，swarm 是"已经实现的"。`team.rs` 暴露的 `AgentRole / TeamPlan / TeamTask` 实际由 `swarm.rs` 内部实现填充（`build_team_milestones` 在 swarm.rs 中），耦合很重。
- 5 套抽象命名容易混淆：lane（任务类）、team（高层 plan）、swarm（执行引擎）、subagent（执行单元）、task_tool（外部接口）。新开发者上手成本高。

### 3.2 `agent/orchestrator.rs`（**2772 行**）—— 主事件循环

**核心类型**：
- `AgentEvent` 枚举（line 34）—— 暴露给 TUI 的所有事件
- `DecisionKind` 枚举（147）—— PlanAction / Clarification / Conflict（与 plan_tracker 匹配）
- `PlanExecutionMode` 枚举（162）
- `PlanOption` 结构（171）+ `generate_plan_options`（177）
- **`PlanStepStatus`（339）—— 与 `tui::plan_tracker::PlanStepStatus`（line 14）重复定义！**这是一个明显的耦合 smell。
- `PlanExecutionState`（348-424）
- `Orchestrator` 结构（425）+ impl（438-2360）—— **2000 行的核心状态机**
- `ReasoningState` impl（2362）

**关键辅助函数**：
- `should_force_swarm(user_input)` —— 按输入文本启发式强制 swarm 模式（line 2506）
- `swarm_patch_approval_details` / `format_swarm_plan_artifact` / `format_plan_artifact`
- `validate_swarm_patch_for_auto_apply` / `validate_patch_applies_cleanly` —— swarm 输出的 patch 自动校验
- `is_unavailable_self_verification` —— 检测 "[Self-verification]" 类标记
- `summarize_parent_context` —— 父 session 上下文摘要（用于 subagent）
- `generate_clarification_questions` —— 主动生成澄清问题

**重要观察**：
- Orchestrator 是 god class（2000 行 impl 块），违背 `docs/architecture_map.md` 的"split into turn/plan/team/tool/event controllers"目标
- 事件流：Orchestrator 通过 `mpsc::UnboundedSender<AgentEvent>` 推送 → TUI 消费 → `event_sink` 持久化 → audit log
- `EmittedStreamDeltas` 是流式 chunk 去重器（避免重复 emit）

### 3.3 `agent/router/mod.rs` —— 复杂度路由器

`ComplexityRouter` + 配套：
- `Route` 枚举（136）—— FastPath / FullAgent / Swarm 类似分流
- `ComplexityLabel` 枚举（146）
- `ReasonCode` 枚举（166）—— 路由理由（用于解释）
- `RiskFlag` 枚举（209）
- `ComplexityAssessment` 结构（224）
- 私有 `classifier.rs` + `rules.rs` + `tests.rs`
- 私有：`determine_route_from_score` / `merge_results` / `build_explanation`

**功能**：按输入特征（关键词 / 文件数量 / 模式匹配）算复杂度分数，决定 fast path 还是触发完整 agent loop 还是 swarm。这是 octocode 一个**明显的差异化设计**——多数 CLI 没有显式路由层。

`RouterConfig`：`router_enabled / router_conservative / router_use_model / router_simple_threshold / router_confidence_threshold`。

### 3.4 `agent/{background, bus, checkpoints, context, decomposer, event_sink, prompt_builder, reasoning, supervisor, tool_loop, utils}` —— 11 个支持模块

未细读签名，但暴露：
- `BackgroundQueue / BackgroundTaskSnapshot / TaskStatus` —— 后台任务队列（与 `cli task` 子命令的"local planned tasks"配套）
- `Message / MessageBus / MessagePayload` —— 内部消息总线
- `Supervisor` —— 顶层监督者（与 Orchestrator 关系待查）
- `prompt_builder` —— prompt 拼装
- `reasoning` —— reasoning（DeepSeek thinking mode）状态机
- `tool_loop` —— ReAct 循环
- `event_sink` —— 事件持久化
- `decomposer` —— 任务分解
- `checkpoints` —— 内存级 undo（不持久化，这是 docs 提到要修的点）

### 3.5 `policy/approvals.rs`（724 行）—— 策略决策中心

- **`PolicyAction`：4 个** —— Allow / Deny / **AskOnce** / **AskSession**（注意 UI 上只显示 a/s/d 三个键，AskSession 通过 [s] 触发）
- **`RiskLevel`：7 档**（与 UI 审批弹窗完全一致）
- `SAFE_READ_TOOLS` 写死 7 个：`read_file / list_dir / search_files / search_code / git_status / git_diff / git_log`
- `evaluate_tool(tool_name, arguments, project_root, policy)` —— 单点决策入口：
  - 先过 `BehavioralPerimeter::check_tool_call` → 命中即 deny Blocked
  - 按 tool name 走分支，每种 tool 类型有独立逻辑
  - Read：调 `evaluate_path_risk` 判断 SafeRead/SensitiveRead/Blocked
  - Search/Git read：直接 Allow（除非 perimeter 命中）
  - Network 工具：受 `policy.network_access` 控制
  - Write：调 `write_paths_decision`（先查 blocked，再查 workspace_safe + autonomy）
  - apply_patch：解析 patch 提取 path，然后等同 write 决策
  - run_command：先 `contains_dangerous_pattern` → deny；再 `requires_network` → deny；最后看 `autonomy_level.auto_local_commands()` 或 `require_approval_for_command`
- `PolicyConfig` 关键 flag：
  - `auto_approve_safe_read / auto_mode`
  - `network_access`
  - `require_approval_for_write / require_approval_for_command`
  - `block_protected_paths`
  - `autonomy_level: AutonomyLevel`（Low / Medium / High，各有方法 `auto_local_commands() / auto_workspace_writes()`）

**亮点**：
- 决策状态机相对完整（4 × 7 × 多 boolean × autonomy 三档）
- patch 解析能提取受影响 path 再决策（apply_patch_approval_lists_affected_paths 测试覆盖）
- block_protected_paths 读用户配置的 protected pattern（不 hardcode）

**问题**：
- 决策是"per-call"，没有 session 级 approval cache（虽然 `AskSession` 设计了但未看到 store）
- 没有"白名单命令"/"白名单路径"机制（Claude Code 的 `permissions.allow`）
- 没有 OS sandbox 集成（macOS Seatbelt / Linux Landlock / Docker），`sandbox.rs` 只是 config

### 3.6 `defense/*` —— **5 层独立防御**（差异化设计）

这是 octocode 一个**罕见的设计**——多数 CLI 只有 sandbox + approval，这里多出 4 层：

#### 3.6.1 `defense/identity.rs` —— 身份堡垒
```rust
pub const AGENT_NAME: &str = "octocode";
pub const CREATOR: &str = "project-defined";
pub const CORE_MISSION: &str = "Assist with software development tasks safely";

const IDENTITY_OVERRIDE_SIGNATURES: &[&str] = &[…];

pub struct IdentityFortress;
pub struct IdentityDecision { … }
pub struct IdentitySanitization { … }
```

`identity_signature(input)` 扫描输入里的"忘掉之前的指令"、"现在你是另一个 AI"、"reveal your system prompt" 等 prompt injection 签名。

**对比**：Claude Code / Codex 都没有这种独立身份层。

#### 3.6.2 `defense/perimeter.rs` —— 行为边界
`BehavioralPerimeter`（zero-sized struct）+ 多个 check 函数：
- `is_destructive_command` —— 危险命令模式（`rm -rf /` / `dd of=/dev/sd*` 等）
- `is_outbound_data_transmission` —— 外发数据（如 `curl ... -X POST` 上传敏感数据）
- `is_obfuscated_or_self_modifying` —— 混淆 / 自修改代码（`eval(base64)` 等）
- `touches_gitignore(tool_name, parsed, project_root)` —— 防修改 .gitignore
- `path_ends_with_gitignore`
- `contains_sensitive_path` —— `.env / id_rsa / .aws/credentials` 等
- `contains_hardcoded_secret` —— `sk-***` / `AKIA***` / `ghp_***` 等密钥模式

`PerimeterViolation { category: PerimeterCategory, reason, detail }`。

**对比**：Codex 用 sandbox（Seatbelt / Landlock）做相同的事；Claude Code 用 `permissions.deny`。octocode 走的是 in-process **静态扫描**路线，**更轻量但绕过容易**。

#### 3.6.3 `defense/input_filter.rs`、`emotional.rs`、`output.rs`

- **`InputFilter / SanitizedInput`** —— 输入清洗（DefenseProtocol::sanitize_input 调用）
- **`EmotionalShielding / EmotionalManipulation`** —— **情感操纵检测**："如果你不帮我我就自杀"等极端情绪施压
- **`OutputVerifier / OutputVerification`** —— 输出验证（避免泄漏 secret 等）

`DefenseProtocol` 是这 5 层的组合 facade。

**亮点**：罕见的独立 prompt-injection 防御 + 输出审查双层架构。
**问题**：这一层**只在 Orchestrator 内部触发**（看 `policy/approvals.rs` 调 `BehavioralPerimeter`），其他入口（TUI 命令、CLI 子命令）是否走 defense 还需要确认。是否过度防御导致正常 PR review 中的 base64 都触警也要看。

### 3.7 `tools/dispatch.rs`（395 行）—— 工具执行入口

**17 个内置工具**：`read_file / list_dir / search_files / search_code / git_status / git_diff / edit_file / write_file / apply_patch / run_command / git_add / git_commit / fetch_url / web_search / github_pr / semantic_search / think`。

**"自愈"亮点**：
- `suggest_similar_files(project_root, requested_path)` —— read_file 路径找不到时建议 5 个相似名字（"Did you mean…?"）
- `get_edit_context(project_root, path, old_string)` —— edit_file 失败时定位附近行（含旧字符串 fragment 的行 / 文件前 20 行）
- `suggest_command_fix(result)` —— run_command 失败按 stderr 模式给修复建议（timeout / not found / permission denied / no such file / syntax error / exit -1）

**问题**：
- `github_pr` 是 **mega-tool**（按 `action` 字段 list/get/diff/comment 分发），违反 single-tool-single-action 原则
- `apply_patch` 走 `crate::workspace::apply::apply_patch`（V4 风格的 `*** Begin Patch / *** Update File` 格式）；同时 `edit_file` 是 Claude 风格的 old_string/new_string —— 两个并存
- `think` 工具直接 echo + 💭 emoji —— 模仿 Anthropic think，但没有"思考摘要进 reasoning 缓冲"等深度集成
- 内置工具没有 `notebook_edit / multi_edit / glob` 等 Claude Code 已有的常用 tool

### 3.8 `mcp/*` —— **只支持 stdio**（重大差距）

mod.rs 注释明确：
> Provides a client and registry for connecting to MCP servers over **stdio**.

- `mcp/client.rs`：`McpClient / McpServerConfig`
- `mcp/protocol.rs`：MCP wire protocol（版本待查）
- `mcp/registry.rs`：`McpRegistry`

**对比**：Claude Code / Codex 都支持 stdio + http/sse + bearer/OAuth；octocode 只有 stdio。**这是后续改造 P0 项**。

### 3.9 `storage/config.rs`（706+ 行）—— **12+ 配置节**

```
Config
├── ModelConfig: default_model / heavy_model / thinking_mode + lanes (default/plan/fim)
├── ExecutionConfig
├── SearchConfig: engine / max_results / max_context_tokens
├── CacheConfig
├── PolicyConfig: network_access / require_approval_* / autonomy_level / command_timeout / protected paths
├── HooksConfig
├── PathsConfig: protected: Vec<String>
├── UiConfig: language / theme / renderer / show_cache_hud / ...
├── TelemetryConfig
├── RouterConfig: enabled / conservative / use_model / simple_threshold / confidence_threshold
├── SubagentConfig: enabled / swarm_enabled / max_parallel / auto_decompose / write_requires_approval / command_requires_approval / default_model
├── ProfileConfig: 多 profile 切换
├── McpConfig: enabled + servers: HashMap<name, McpServerEntryConfig>
```

`AutonomyLevel`（146）枚举 + 方法 `auto_local_commands() / auto_workspace_writes()`。

**亮点**：
- 配置层级**非常成熟**（比 Codex 早期版本细），12+ 节，每节都有 default 函数
- `RouterConfig` 把复杂度路由暴露成可配置——这点比 Claude Code 更进一步
- `ProfileConfig` 支持 profile 切换

**问题**：
- 缺少 user-level vs project-level 优先级机制（Claude Code 的 settings.json + ~/.claude/settings.json 分层）
- 没有看到 `permissions.allow / permissions.deny` 等显式白名单（Claude Code 用这个做 fine-grained 控制）
- 没有 schema 验证（TOML 写错只能在 runtime 报）

### 3.10 `commands/mod.rs`（28071 tokens / ~1400 行）—— **48 个斜杠命令**！

这是**大发现**。完整命令清单（按分类）：

| 类别 | 命令 |
|---|---|
| 基础 | `/clear /copy /undo /image /help` |
| 工作流 | `/commit /test /fix /explain /review /wiki /readiness_report /security_review /simplify /run /ask /plan /search` |
| 状态查看 | `/status /context /cwd /usage /doctor` |
| 工具管理 | `/mcp /model /sessions /memory /init /compact /add_dir /commands /skills /hooks /plugins /agents /tasks /schedule` |
| 设置 | `/theme /tui /settings /mode /permissions /config /statusline /auto /yolo /swarm` |
| 撤销 | `/checkpoint /restore` |

**关键发现**：
- 已有 `/hooks`、`/skills`、`/plugins`、`/statusline`、`/agents`、`/tasks`、`/permissions`、`/init`、`/compact`、`/sessions`、`/memory` —— 这些是 Claude Code 的核心扩展点！
- 已有 `/swarm`、`/auto`、`/yolo`、`/mode` —— autonomy 控制有多重入口
- 已有 `/copy`（复制 transcript）、`/undo`、`/restore`、`/checkpoint` —— 撤销机制完整

**问题**：
- **功能广度上 octocode 已经超过 Codex CLI、接近 Claude Code**（数量上甚至更多）
- 但**实现深度未知**——比如 `cmd_skills` 有没有像 Claude Code 那样的 progressive disclosure SKILL.md 加载？`cmd_hooks` 是否真的钩进 lifecycle？这是后续 Phase 2 验证的关键
- input.rs **没有触发 slash command panel**——意味着这 48 个命令对用户**不可发现**（必须记住 `/x`），UI 没拉通

### 3.11 `tui/app.rs`（3600+ 行）—— TUI 主事件循环

**核心类型**：
- `TuiApp`（line 65）—— 全局状态容器
- `DecisionPrompt`（128）
- `SwarmViewState`（136）
- **`InteractionMode`（151）+ `RendererMode`（159）—— 两套 mode 系统！**InteractionMode 估计是 chat/plan/run/review，RendererMode 估计是 classic/fullscreen。混用同一个词容易乱
- `ApiKeyEntry`（215）+ `ApiKeyState`（221）
- `TuiAction`（2558）—— 按键解码后的 action enum
- `PreviewSnapshotScenario`（2616）+ `render_preview_snapshot`（2623）

**已经实现但 input.rs 不知道**：
- `mention_prefix_at_cursor(text, cursor_pos)` —— **@ 提及检测**！在 app.rs 层做
- `file_mention_candidates(root, prefix, limit)` —— @ 文件补全候选
- `collect_file_mention_candidates / should_skip_mention_dir / common_string_prefix` —— 完整的 @ 补全实现

**已经实现的 shell mode**：
- `shell_command_from_input(input) -> Result<Option<String>, &'static str>` —— `! ...` 前缀解析
- `is_swarm_cancel_command(input)` —— 中止 swarm 的特殊输入
- `shell_tool_arguments / shell_tool_call / start_local_shell_command` —— 直接构造 run_command 工具调用

**选项面板的输入解析**：
- `option_shortcut_index(c, option_count)` —— 1-9 / a-z 快捷键转 index
- `try_match_option(input, options)` —— 按词匹配 option（如 "yes" 匹配带 "yes" 的选项）

**活动标题摘要**（4 个变体）：
- `summarize_task_title / summarize_cjk_task_title / summarize_cjk_keywords / summarize_latin_task_title / summarize_plan_step / summarize_agent_plan_step / trim_title_punctuation`
- 这是给 status_bar 的"⠴ <task title>... (6s · ...)" 那行用的

**Brewed for 之谜**：`append_brewed_line` 在 app.rs 中（line 3484）——说明 "Brewed for N" 是 octocode 自己的 UI 信号（不是从模型输出抄来的），表示"模型刚刚 'pondered' 了 N 秒"。视觉灵感可能来自 GitHub Copilot 的 "thinking" 标记。

**run_tui(...)（3587）—— TUI 主入口**：通过 cli_entry 调用，构造 TuiApp 后进入 event loop。

---

## 4. **完整版**痛点 / 改造重点清单（升级版，21 项）

排序按"可见度 × 修复 ROI"：

### A. UI 视觉与一致性（6 项，与原 11 痛点合并）
1. **ASCII wordmark 不像 "DeepSeek"** + 无 narrow/wide 自适应
2. **底部 statusline 11 个彩色 chip 配色完全 bypass 主题** —— 切 Light 主题视觉断裂；信息密度过高
3. **diff_viewer 强制 BG_DEEP 底色** —— 不跟主题
4. **syntax_highlight 强制 `base16-ocean.dark`** —— Light 主题代码块对比异常
5. **file_tree 用老色常量** —— 不跟 palette
6. **theme palette 与老 color 常量两套并存** —— BG_CARD / FG_PRIMARY / ACCENT_AMBER 等大量遗留入口要么接入 palette 要么删除

### B. UI 交互 & 可发现性（5 项）
7. **input.rs 完全无 inline picker** —— slash command panel 已实现但 input 不触发；@ 提及补全在 app.rs 实现但 input 不显示；Ctrl-R 历史搜不存在；paste detection 不存在
8. **48 个斜杠命令对用户不可发现** —— 没有 `/commands` 自动补全 UI
9. **欢迎页 3 个 starter prompt 硬编码** —— 不按项目动态生成
10. **设置面板是占位** —— 提到 Powerline / Capsule / 启动 logo 动画 / 音效都是规划但未实现
11. **status_bar 与 statusline 信息冗余** —— 活动指示器在两处显示

### C. UI 渲染深度（3 项）
12. **markdown parser 不识别列表 / `*italic*` / 链接 / image** —— 对话区 markdown 渲染先天受限
13. **table card mode 把第二列固定当 title** —— hardcoded 假设
14. **should_hide_transcript_line 用字符串前缀** —— fragile，重命名上游标记就断

### D. 后端架构（4 项）
15. **5 套并行的多 agent 抽象（subagent / team / swarm / lanes / task_tool）** —— 命名混乱，team 与 swarm 是同一问题的两个解，应该收敛
16. **`PlanStepStatus` 在 orchestrator 与 plan_tracker 中双重定义** —— typed event 应该单 source of truth
17. **Orchestrator 是 god class（2000 行 impl）** —— 违反 architecture_map 的拆分目标
18. **MCP 只支持 stdio** —— Codex / Claude Code 都支持 stdio + http/sse；这是最大单点功能差距

### E. 安全 / 可靠性（3 项）
19. **defense 层只在 Orchestrator 入口触发** —— TUI / CLI 子命令是否过 defense 待验证
20. **没有 session 级 approval 持久化** —— `PolicyAction::AskSession` 设计了但 store 未看到
21. **rollback 只在内存** —— docs/openai_codex_deepseek_tui_comparison.md 明确提到要 durable rollback

---

## 5. 现状 vs 竞品（粗略印象，待后续 6 份调研报告精化）

| 维度 | octocode 现状 | 与竞品差距 |
|---|---|---|
| **CLI 命令面** | 17 个子命令 + 48 个 slash | **比 Codex 多，约等于 Claude Code，超过 Gemini** |
| **多 agent 协同** | swarm / team / subagent 三层并行 | 架构最复杂；但**抽象耦合多**，需要简化 |
| **审批与策略** | 4 actions × 7 risks + autonomy 三档 + perimeter | 状态机比 Codex 完整；缺白名单 + 持久化 |
| **defense / prompt-injection 防御** | 5 层独立 | **罕见的差异化**，竞品多数没有 |
| **复杂度路由** | ComplexityRouter + Router config | **罕见**，Claude Code/Codex 隐式做；octocode 显式且可配 |
| **MCP** | 只 stdio | **重大差距**：Claude Code/Codex 支持 http/sse + auth |
| **theme 系统** | 3 套 palette + 自动检测 | 与 Codex/Gemini 持平 |
| **subagent UI** | SubagentCard with R/W/token/duration | 与 Claude Code Task tool UI 接近，**缺 "展开 / 取消" 控制** |
| **diff 渲染** | 文件 list + syntect + status icon | 与 Codex 持平；缺 side-by-side、hunk 折叠 |
| **欢迎页** | 响应式 4 布局 + skills/MCP/AGENTS.md 状态行 | **比 Codex 信息密度大**；starter 硬编码 |
| **statusline** | 11 个彩色 chip | **比所有竞品都密**；不跟主题；¥ CNY 硬编码 |
| **shell 集成 (! 模式)** | 实现 | 与 Gemini/Claude Code 持平 |
| **@ mention 补全** | 后端有，UI 没拉通 | **后端逻辑早就写了，input.rs 没渲染** |
| **slash command panel** | 实现，但 input 不触发 | **代码就在隔壁** |
| **历史搜索 (Ctrl-R)** | 未实现 | 与 Codex 有差距 |
| **OS sandbox 集成** | 只静态扫描 perimeter | **明显落后** Codex (Seatbelt/Landlock) |
| **hooks/skills/plugins** | 命令存在，深度未知 | 需 Phase 2 验证实现是否到位 |
| **会话持久化** | sessions + transcripts + events.jsonl | 与 Codex 持平 |
| **OAuth login** | 只 API key + keyring | 与 Codex (ChatGPT login) / Claude Code (Pro/Max login) 有差距 |

---

## 6. Phase 2 改造的 "动手优先级建议"（待方案三件套敲定）

> 这个章节只是**建议雏形**——最终顺序要在拿到 6 份竞品调研报告后回头修订。

**P0：UI 拉通（小而见效）**
1. input.rs 接入 slash command panel + @ mention picker + Ctrl-R 历史搜
2. statusline 接入主题 palette（移除硬编码 RGB）
3. diff_viewer / file_tree / syntax_highlight 切到主题色
4. 修 ASCII wordmark 设计
5. 删除老 color 常量，统一走 palette

**P0：功能闭环**
6. MCP 加 http/sse 传输
7. session 级 approval 持久化 + UI 展示已批准列表
8. checkpoints 持久化（rollback 跨会话可用）
9. markdown parser 升级（列表 / 行内样式 / 链接）

**P1：架构收敛**
10. team / swarm / subagent 合并到单一 `TeamPlan → TeamRun → TeamTask → AgentRun` 流程
11. `PlanStepStatus` 单 source of truth
12. Orchestrator god class 拆分（turn / plan / team / tool / event 五个 controller）

**P1：可发现性**
13. 欢迎页 starter 动态化（按语言 / git 状态 / 最近 session）
14. `/commands` 弹窗内置在 input.rs（输入 `/` 自动弹出 48 个命令）
15. settings_panel 升级为可编辑 UI

**P2：差异化深耕**
16. complexity router 接入更多信号 + 可视化决策路径
17. defense 层暴露到 settings（可关，可调整严苛度）
18. hooks / skills / plugins 深度对齐 Claude Code 标准

**P2：分发与生态**
19. 发布 release artifacts（npm / Homebrew / Scoop / GitHub Release）
20. OAuth 登录（DeepSeek 账户而非 API key）
21. IDE 扩展（VS Code / JetBrains）

---

## 7. 还没读的部分（task #1 剩余 ~10%）

下次需要补完的：
- [ ] `agent/orchestrator.rs` 主 impl 块内具体方法签名（已扫 fn 头，未深入业务逻辑）
- [ ] `defense/perimeter.rs` 行内 `PerimeterCategory` 枚举具体值
- [ ] `mcp/protocol.rs` —— MCP 版本
- [ ] `deepseek/client.rs` —— DeepSeek API surface（thinking / FIM / cache / migration）
- [ ] `storage/sessions.rs / transcripts.rs / events.rs` —— 持久化 schema
- [ ] `cli/*.rs` 子命令实现（review / agent / mission / features 等）
- [ ] `agent/orchestrator.rs` 内详细业务（patch validation / plan execution / 流式输出）—— 这部分通过实际改造时再深入

我评估 task #1 现状摸底已经做到 **~85%**，达到了"足够派出竞品调研 agent 不会瞎对比"的标准。**可以进入 Phase 1 下一阶段：启动第一个竞品调研 agent**。

---

*Last updated: 2026-05-13.*

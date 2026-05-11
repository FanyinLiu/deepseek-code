# DeepSeek-Code

DeepSeek 原生编程代理软件 — 围绕 DeepSeek API 特有能力设计的本地编程代理。

## 安装

```bash
# 从源码目录安装日常命令 ds
cd deepseek-code
cargo install --path . --bin ds --force

# 也可以安装兼容长别名 dscode
cargo install --path . --bin dscode --force

# 不安装也可以直接运行
cargo run --bin ds -- tui

# 构建三个二进制：deepseek-code + dscode + ds
cargo build --release
```

## 快速开始

```bash
# 登录，也可以首次打开 TUI 后直接粘贴 API key
deepseek-code login --api-key sk-xxx

# 诊断
deepseek-code doctor

# 对话
deepseek-code chat "解释这个项目"

# 提问（含项目搜索上下文）
deepseek-code ask "auth middleware 在哪里"

# 搜索代码
deepseek-code search "orchestrator"

# 计划模式（只读分析）
deepseek-code plan "修复登录失败的问题"

# 执行任务
deepseek-code run "列出 src 目录"

# 交互式 TUI（推荐）
ds

# 兼容显式子命令
ds tui
dscode tui

# 不安装时从源码直接启动 TUI
cargo run --bin ds -- tui

# 会话管理
deepseek-code resume
deepseek-code export <session-id>
```

`ds` 是面向日常使用的短命令，`dscode` 保留为兼容长别名。如果 PowerShell 提示找不到
`ds`，请确认 Cargo 的 bin 目录在 PATH 中，通常是 `%USERPROFILE%\.cargo\bin`。

## 核心能力

- **DeepSeek-native**: thinking/reasoning_content 生命周期、工具调用、上下文缓存、FIM
- **Plan Mode**: 只读分析 → 生成计划 → 风险审查 → 用户确认后执行
- **安全审批**: 路径保护、命令沙箱、风险分级、每次审批/会话授权
- **会话持久化**: 项目级会话保存、恢复、导出、分叉
- **缓存可视化**: 实时显示缓存命中率、token 用量、预估费用
- **本地搜索**: ripgrep 全文搜索、文件 glob、符号搜索

## 配置

```
~/.deepseek-code/config.toml     # 用户全局
./.deepseek-code/config.toml     # 项目共享（提交到 git）
./.deepseek-code/local.toml      # 本机私有（gitignore）
```

## 架构

```
deepseek-code/
├── src/
│   ├── agent/       # 核心循环、推理管理、工具循环
│   ├── deepseek/    # API 客户端、SSE 流解析、模型管理
│   ├── plan/        # 计划生成、审查、执行
│   ├── search/      # 本地搜索、上下文打包、rerank
│   ├── tools/       # 工具实现（读/写/编辑/命令）
│   ├── policy/      # 审批引擎、沙箱、路径保护、脱敏
│   ├── storage/     # 会话/配置/凭证持久化
│   ├── workspace/   # Git/diff/apply/路径管理
│   ├── tui/         # 终端 UI
│   └── cli/         # CLI 子命令
└── tests/           # 集成测试
```

## 许可证

MIT

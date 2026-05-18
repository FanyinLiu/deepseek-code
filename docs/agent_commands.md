# Agent Commands

`octocode agent` turns the existing subagent runtime into an explicit CLI product
surface. It lists built-ins, shows their defaults, creates project custom
agents, validates agent files, and can run a named agent on a task.

## Commands

```bash
octocode agent list
octocode agent list --json
octocode agent show code-reviewer
octocode agent show security-auditor --json
octocode agent run code-explorer "explain src/agent" --focus src/agent
octocode agent create my-auditor --template auditor
octocode agent validate --all
octocode agent validate my-auditor --json
```

## Built-In Agents

| Name | Default mode | Default model | Purpose |
|---|---|---|---|
| `general-purpose` | `default` | `deepseek-v4-flash` | General task agent |
| `code-explorer` | `read_only` | `deepseek-v4-flash` | Read-only codebase exploration |
| `code-reviewer` | `read_only` | `deepseek-v4-flash` | Read-only code review |
| `planner` | `read_only` | `deepseek-v4-flash` | Read-only implementation planning |
| `test-runner` | `accept_edits` | `deepseek-v4-flash` | Focused test execution and failure analysis |
| `architect` | `read_only` | `deepseek-v4-pro` | API and system design review |
| `security-auditor` | `read_only` | `deepseek-v4-pro` | Read-only security audit with VETO reporting |

The security auditor is intentionally constrained to:

```text
read_file, list_dir, search_files, search_code, git_status, git_diff
```

## Custom Agents

Custom agents live under:

```text
.octocode/agents/<name>.md
```

Files use markdown with TOML frontmatter:

```markdown
---
subagent_type = "code-reviewer"
allowed_tools = ["read_file", "list_dir", "search_code", "git_diff"]
permission_mode = "read_only"
model = "deepseek-v4-pro"
max_turns = 10
---

# Reviewer

Review only. Do not edit files.
```

Templates:

```text
explorer | reviewer | auditor | tester | planner | writer
```

Validation checks that frontmatter parses, allowed tools are known, permission
mode/model values deserialize, the prompt body is non-empty, and risky prompt
phrases are reported as warnings.

## Non-Interactive Run Behavior

`octocode agent run` uses the real subagent executor. If a tool requires interactive
approval in this plain terminal mode, the command denies the request rather than
auto-approving it. Use read-only agents for unattended inspection and keep
write-capable work behind normal project policy.

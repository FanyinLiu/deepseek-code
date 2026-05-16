use crate::deepseek::ExecutionLane;

/// Classify a user input into the appropriate execution lane.
#[must_use]
pub fn classify_task(input: &str) -> TaskClass {
    let input_lower = input.trim().to_lowercase();

    // Plan mode triggers
    if input_lower.starts_with("/plan")
        || input_lower.starts_with("plan ")
        || input_lower.contains("plan how to")
        || input_lower.contains("create a plan")
    {
        return TaskClass::Plan;
    }

    // Search triggers
    if input_lower.starts_with("/search")
        || input_lower.starts_with("search ")
        || input_lower.starts_with("find ")
        || input_lower.starts_with("where is")
        || input_lower.starts_with("locate ")
        || input_lower.starts_with("/ask")
        || input_lower.starts_with("/review")
        || has_local_context_intent(&input_lower)
    {
        return TaskClass::Search;
    }

    // Execution triggers
    if input_lower.starts_with("/run")
        || input_lower.contains("run the tests")
        || input_lower.contains("execute")
    {
        return TaskClass::Execute;
    }

    // Edit triggers — implies tool loop
    if input_lower.starts_with("fix ")
        || input_lower.starts_with("refactor ")
        || input_lower.starts_with("implement ")
        || input_lower.starts_with("add ")
        || input_lower.starts_with("change ")
        || input_lower.starts_with("modify ")
        || input_lower.starts_with("rewrite ")
        || input_lower.starts_with("remove ")
        || input_lower.starts_with("delete ")
        || input_lower.starts_with("update ")
    {
        return TaskClass::Execute;
    }

    // Chat — default
    TaskClass::Chat
}

fn has_local_context_intent(input: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "read file",
        "read this file",
        "read ",
        "cat ",
        "open file",
        "local file",
        "computer file",
        "computer folder",
        "my computer",
        "absolute path",
        "local folder",
        "folder",
        "show file",
        "show ",
        "list files",
        "list dir",
        "directory",
        "project structure",
        "workspace",
        "codebase",
        "inspect code",
        "search code",
        "find bug",
        "analyze repo",
        "读取",
        "读一下",
        "查看",
        "看一下",
        "打开文件",
        "电脑文件",
        "电脑里的文件",
        "电脑里",
        "电脑里面",
        "电脑上的",
        "我的电脑",
        "本机文件",
        "本地文件",
        "本地目录",
        "本地文件夹",
        "绝对路径",
        "文件夹",
        "目录",
        "项目结构",
        "目录结构",
        "文件结构",
        "列出文件",
        "搜索文件",
        "搜索代码",
        "检查代码",
        "分析仓库",
        "分析项目",
        "找 bug",
        "找bug",
        "排查 bug",
        "排查bug",
    ];
    NEEDLES.iter().any(|needle| input.contains(needle))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskClass {
    Chat,
    Search,
    Plan,
    Execute,
}

impl TaskClass {
    #[must_use]
    pub fn default_lane(&self) -> ExecutionLane {
        match self {
            Self::Chat => ExecutionLane::ChatNonThinking,
            Self::Search => ExecutionLane::ToolLoopThinking,
            Self::Plan => ExecutionLane::PlanThinking,
            Self::Execute => ExecutionLane::ToolLoopThinking,
        }
    }

    #[must_use]
    pub fn needs_thinking(&self) -> bool {
        matches!(self, Self::Search | Self::Plan | Self::Execute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_file_requests_use_tool_lane() {
        assert_eq!(classify_task("你能读取电脑里的文件吗"), TaskClass::Search);
        assert_eq!(classify_task("能读取我的电脑文件吗"), TaskClass::Search);
        assert_eq!(
            classify_task("检测一下电脑里面 ds文件夹的 deekseep code 文件夹"),
            TaskClass::Search
        );
        assert_eq!(classify_task("read my computer file"), TaskClass::Search);
        assert_eq!(classify_task("看一下项目结构"), TaskClass::Search);
        assert_eq!(classify_task("search code for ToolLoop"), TaskClass::Search);
        assert_eq!(classify_task("hello"), TaskClass::Chat);
        assert_eq!(
            TaskClass::Search.default_lane(),
            ExecutionLane::ToolLoopThinking
        );
    }
}

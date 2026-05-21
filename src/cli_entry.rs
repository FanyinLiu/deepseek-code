use std::{io::IsTerminal, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{cli, tui};

#[derive(Parser)]
#[command(
    name = "octo",
    version,
    about = "Multi-model, multi-agent coding CLI",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override project root directory
    #[arg(global = true, short = 'C', long)]
    project_root: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check API connectivity, auth, and configuration
    Doctor,

    /// Store provider API key in system keyring
    Login {
        /// Provider API key
        #[arg(short, long)]
        api_key: Option<String>,
    },

    /// Start an interactive chat session or send a one-shot prompt
    Chat {
        /// The prompt to send
        prompt: Option<String>,

        /// Enable thinking mode
        #[arg(short, long)]
        thinking: bool,

        /// Model override (pro, flash)
        #[arg(short, long)]
        model: Option<String>,

        /// Resume a specific session by ID
        #[arg(long)]
        session: Option<String>,

        /// Output format: text, json, stream-json
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Text)]
        output_format: OutputFormatArg,

        /// Tool approval policy: ask (prompt), allow, or deny
        #[arg(long, value_enum)]
        tool_approval: Option<ToolApprovalArg>,

        /// Enable tool approval for every request without prompts
        #[arg(short = 'y', long = "auto-approve")]
        auto_approve: bool,
    },

    /// Ask a question with search context (read-only, no edits)
    Ask {
        /// The question to ask
        question: String,

        /// Output format: text, json, stream-json
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Text)]
        output_format: OutputFormatArg,

        /// Tool approval policy: ask (prompt), allow, or deny
        #[arg(long, value_enum)]
        tool_approval: Option<ToolApprovalArg>,

        /// Enable tool approval for every request without prompts
        #[arg(short = 'y', long = "auto-approve")]
        auto_approve: bool,
    },

    /// Search the project codebase
    Search {
        /// Search query
        query: String,

        /// Search code only (skip file names)
        #[arg(long)]
        code_only: bool,

        /// Maximum results
        #[arg(short, long, default_value = "30")]
        limit: usize,
    },

    /// Generate a plan for a task (read-only, no edits)
    Plan {
        /// The task to plan
        task: String,
    },

    /// Execute a task with tool access and approval
    Run {
        /// The task to execute
        task: String,

        /// Enable thinking mode
        #[arg(short, long)]
        thinking: bool,

        /// Tool approval policy: ask (prompt), allow, or deny
        #[arg(long, value_enum)]
        tool_approval: Option<ToolApprovalArg>,

        /// Enable tool approval for every request without prompts
        #[arg(short = 'y', long = "auto-approve")]
        auto_approve: bool,

        /// Output format: text, json, stream-json
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Text)]
        output_format: OutputFormatArg,
    },

    /// List, resume, or export saved sessions
    Resume {
        /// Session name or ID prefix to resume
        session: Option<String>,
    },

    /// Export a session transcript
    Export {
        /// Session ID to export
        session_id: Option<String>,

        /// Output format: markdown, json, text
        #[arg(short, long, default_value = "markdown")]
        format: String,
    },

    /// Assess task complexity (diagnostics / debug routing)
    Assess {
        /// The task to assess
        task: String,
    },

    /// Run a multi-agent code review over the entire project
    Review {
        /// Maximum parallel reviewers (default: 4)
        #[arg(short, long, default_value = "4", value_parser = parse_positive_usize)]
        parallel: usize,

        /// Max tool-call turns per reviewer (default: 15)
        #[arg(long, default_value = "15")]
        max_turns: u32,

        /// Save report to file instead of printing
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Discover available features and recommended operating modes
    Features {
        #[command(subcommand)]
        command: FeaturesCommands,
    },

    /// Show provider model and thinking capabilities
    Models {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Read, write, and run persistent Octocode goals
    Goal {
        #[command(subcommand)]
        command: GoalCommands,
    },

    /// Inspect self-evolution archive candidates and lineage
    Archive {
        #[command(subcommand)]
        command: ArchiveCommands,
    },

    /// Create repair proposals, validation runs, and repair reports
    Repair {
        #[command(subcommand)]
        command: RepairCommands,
    },

    /// Maintain project knowledge, risk map, and failure memory
    Knowledge {
        #[command(subcommand)]
        command: KnowledgeCommands,
    },

    /// Manage reusable local skills generated from successful repair/evolution runs
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },

    /// Inspect and run self-evolution proposal workflows
    Evolve {
        #[command(subcommand)]
        command: EvolveCommands,
    },
    /// Manage built-in and project custom agents
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },

    /// Manage MCP server configuration and status
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },

    /// List built-in and custom slash commands
    #[command(name = "commands")]
    Catalog {
        #[command(subcommand)]
        command: CommandsCatalogCommands,
    },

    /// Explain resolved configuration and source precedence
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Read or update local editable settings
    Settings {
        #[command(subcommand)]
        command: SettingsCommands,
    },

    /// List, replay, or export saved sessions
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },

    /// Record and inspect long-running mission dry-runs
    Mission {
        #[command(subcommand)]
        command: MissionCommands,
    },

    /// Manage local planned tasks
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },

    /// Start the interactive TUI
    Tui {
        /// Enable thinking mode
        #[arg(short, long)]
        thinking: bool,

        /// Model override (pro, flash)
        #[arg(short, long)]
        model: Option<String>,

        /// Resume a specific session by ID
        #[arg(long)]
        session: Option<String>,
    },

    /// Render a non-interactive TUI snapshot for development inspection
    #[command(hide = true)]
    PreviewTui {
        /// Snapshot width in terminal cells
        #[arg(long, default_value_t = 120)]
        width: u16,

        /// Snapshot height in terminal cells
        #[arg(long, default_value_t = 28)]
        height: u16,

        /// Simulated API key state
        #[arg(long, value_enum, default_value_t = PreviewApiState::Missing)]
        api: PreviewApiState,

        /// Simulated TUI state
        #[arg(long, value_enum, default_value_t = PreviewScenario::Welcome)]
        scenario: PreviewScenario,

        /// Simulated UI theme
        #[arg(long, value_enum, default_value_t = PreviewTheme::Auto)]
        theme: PreviewTheme,

        /// Fixed animation elapsed time in milliseconds
        #[arg(long, default_value_t = 0)]
        elapsed_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PreviewApiState {
    Missing,
    Ready,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PreviewScenario {
    Welcome,
    Workbench,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PreviewTheme {
    Auto,
    Light,
    Dark,
    HighContrast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormatArg {
    Text,
    Json,
    StreamJson,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ToolApprovalArg {
    Ask,
    Allow,
    Deny,
}

impl ToolApprovalArg {
    fn to_policy(self) -> crate::cli::ToolApprovalPolicy {
        match self {
            Self::Ask => crate::cli::ToolApprovalPolicy::Ask,
            Self::Allow => crate::cli::ToolApprovalPolicy::Allow,
            Self::Deny => crate::cli::ToolApprovalPolicy::Deny,
        }
    }
}

fn effective_turn_tool_approval(
    output_format: OutputFormatArg,
    tool_approval: Option<ToolApprovalArg>,
    auto_approve: bool,
) -> crate::cli::ToolApprovalPolicy {
    if auto_approve {
        return crate::cli::ToolApprovalPolicy::Allow;
    }
    if let Some(tool_approval) = tool_approval {
        return tool_approval.to_policy();
    }
    match output_format {
        OutputFormatArg::Text => crate::cli::ToolApprovalPolicy::Ask,
        OutputFormatArg::Json | OutputFormatArg::StreamJson => crate::cli::ToolApprovalPolicy::Deny,
    }
}

impl OutputFormatArg {
    fn turn_output_format(self) -> crate::cli::TurnOutputFormat {
        match self {
            Self::Text => crate::cli::TurnOutputFormat::Text,
            Self::Json => crate::cli::TurnOutputFormat::Json,
            Self::StreamJson => crate::cli::TurnOutputFormat::StreamJson,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum McpTransportArg {
    Stdio,
    Http,
    Sse,
}

impl From<McpTransportArg> for crate::mcp::client::McpTransport {
    fn from(value: McpTransportArg) -> Self {
        match value {
            McpTransportArg::Stdio => Self::Stdio,
            McpTransportArg::Http => Self::Http,
            McpTransportArg::Sse => Self::Sse,
        }
    }
}

#[derive(Subcommand)]
enum TaskCommands {
    /// List local planned tasks
    List,
    /// Add a local planned task
    Add {
        /// Task kind: heartbeat or standalone
        kind: String,
        /// Prompt to run
        prompt: String,
    },
    /// Pause a task
    Pause { id: String },
    /// Resume a paused task
    Resume { id: String },
    /// Run a task now
    Run {
        id: String,
        /// Output format for task execution: text, json, stream-json
        #[arg(long, value_enum)]
        format: Option<OutputFormatArg>,
    },
    /// Show task logs and metadata
    Logs { id: String },
    /// Remove a task
    Rm { id: String },
}

#[derive(Subcommand)]
enum FeaturesCommands {
    /// Show the competitive feature matrix summary
    Matrix {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show local capability and configuration status
    Status {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Recommend an operating mode for a task
    Recommend {
        /// Task description
        task: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum GoalCommands {
    /// Write the default .octocode/goals.toml
    Init {
        /// Replace an existing goals file
        #[arg(long)]
        overwrite: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the effective goal configuration
    Show {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Plan a task graph from the active objective
    Plan {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the latest recorded goal task graph
    Graph {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Set the active objective for future automatic evolution
    SetActive {
        /// Objective id, such as goal_binding
        id: String,
        /// Objective title
        title: String,
        /// Success criterion; repeat for multiple criteria
        #[arg(long = "success")]
        success: Vec<String>,
        /// Target file for generated candidate patches; repeat for multiple targets
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run a self-evolution proof from the active objective
    Run {
        /// Override model for the model-generated candidate patch: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Run full remote validation when the sandbox backend supports it
        #[arg(long)]
        full: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ArchiveCommands {
    /// Show archive counters
    Status {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// List archive candidates with utility scores
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show one archive candidate
    Show {
        /// Candidate id
        candidate_id: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// List lineage events
    Lineage {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum RepairCommands {
    /// Create a repair proposal without editing files
    Propose {
        /// Problem statement
        problem: String,
        /// Proposal title
        #[arg(long)]
        title: Option<String>,
        /// File expected to be relevant; repeat for multiple targets
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run deterministic repair gates and write a report
    Run {
        /// Proposal id from `octo repair propose`
        proposal_id: String,
        /// Run full validation, including tests when available
        #[arg(long)]
        full: bool,
        /// Ask the configured model to generate a candidate unified diff
        #[arg(long)]
        model_patch: bool,
        /// Override model for --model-patch: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Print a repair run report
    Report {
        /// Run id from `octo repair run`
        run_id: String,
    },
    /// Show local repair proposals, runs, and failure-memory count
    Status {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum KnowledgeCommands {
    /// Refresh project.md and ensure risk-map/failure-memory files exist
    Update {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a knowledge asset
    Show {
        /// Asset to show: project, risk-map, or failures
        topic: KnowledgeTopicArg,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum KnowledgeTopicArg {
    Project,
    RiskMap,
    Failures,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// List local skills
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show one local skill
    Show {
        /// Skill id
        skill_id: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a draft skill from a repair run
    Add {
        /// Repair run id from `octo repair run`
        run_id: String,
        /// Override generated skill name
        #[arg(long)]
        name: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run structural checks for a local skill
    Test {
        /// Skill id
        skill_id: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum EvolveCommands {
    /// Inspect local evolution proposals, runs, applies, and rollbacks
    Inspect {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a self-evolution proposal without editing source files
    Propose {
        /// Capability area this evolution should improve
        #[arg(long)]
        area: Option<String>,
        /// Source files the generated patch is expected to touch
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        /// Proposal title
        #[arg(long)]
        title: Option<String>,
        /// Problem statement
        #[arg(long)]
        problem: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate a candidate patch run for an evolution proposal
    Patch {
        /// Proposal id from `octo evolve propose`
        proposal_id: String,
        /// Use real model-backed planner/implementer/safety reviewer agents
        #[arg(long)]
        model_agents: bool,
        /// Override model for model-backed agents: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Convert an evolution proposal into a repair-backed validation run
    Repair {
        /// Proposal id from `octo evolve propose`
        proposal_id: String,
        /// Ask the configured model to generate a repair candidate patch
        #[arg(long)]
        model_patch: bool,
        /// Override model for --model-patch: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Run full repair validation
        #[arg(long)]
        full: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Prove a concrete self-improvement task through repair and validation
    ProveSelf {
        /// Concrete self-improvement task to prove through repair + remote sandbox
        #[arg(long)]
        task: String,
        /// Low-risk Octocode source files the proof patch is expected to touch
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        /// Override model for the model-generated candidate patch: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Run full remote validation when the sandbox backend supports it
        #[arg(long)]
        full: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run self-evolution from the active project goal
    FromGoal {
        /// Override model for the model-generated candidate patch: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Run full remote validation when the sandbox backend supports it
        #[arg(long)]
        full: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run repeated goal-driven self-evolution rounds
    Benchmark {
        /// Number of goal-driven self-evolution rounds to run
        #[arg(long, default_value_t = 3, value_parser = parse_positive_usize)]
        rounds: usize,
        /// Override model for the model-generated candidate patch: pro or flash
        #[arg(long)]
        model: Option<String>,
        /// Run full remote validation when the sandbox backend supports it
        #[arg(long)]
        full: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show evolution memory and failure history
    Memory {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run validation gates for a generated evolution patch
    Test {
        /// Run id from `octo evolve patch`
        run_id: String,
        /// Run full validation, including the full test suite when available
        #[arg(long)]
        full: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Apply a tested evolution patch to the current tree
    Apply {
        /// Run id from `octo evolve patch`
        run_id: String,
        /// Allow applying a high-risk patch after manual review
        #[arg(long)]
        allow_high_risk: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Roll back a previously applied evolution patch
    Rollback {
        /// Apply id from `octo evolve apply`
        apply_id: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show local self-evolution status
    Status {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AgentCommands {
    /// List built-in and project custom agents
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show one agent's configuration and prompt
    Show {
        /// Agent name
        name: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run one named agent on a task
    Run {
        /// Agent name
        name: String,
        /// Task description
        task: String,
        /// Focus path for the agent
        #[arg(long)]
        focus: Option<PathBuf>,
        /// Max tool-call turns
        #[arg(long)]
        max_turns: Option<u32>,
        /// Model override (pro, flash)
        #[arg(long)]
        model: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a custom markdown agent from a template
    Create {
        /// New agent name
        name: String,
        /// Template name
        #[arg(long)]
        template: AgentTemplateArg,
    },
    /// Validate custom agent files or built-ins
    Validate {
        /// Agent name to validate
        name: Option<String>,
        /// Validate every built-in and custom agent
        #[arg(long)]
        all: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AgentTemplateArg {
    Explorer,
    Reviewer,
    Auditor,
    Tester,
    Planner,
    Writer,
}

#[derive(Subcommand)]
enum McpCommands {
    /// List configured MCP servers
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show one configured MCP server
    Get {
        /// Server name
        name: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show MCP status; add --connect for a live server check
    Status {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Connect to configured servers and discover tools
        #[arg(long)]
        connect: bool,
    },
    /// Inspect and call tools advertised by one MCP server
    Tools {
        #[command(subcommand)]
        command: McpToolsCommands,
    },
    /// Inspect and read resources advertised by one MCP server
    Resources {
        #[command(subcommand)]
        command: McpResourcesCommands,
    },
    /// Add or replace a project-local MCP server
    Add {
        /// Server name
        name: String,
        /// Transport kind
        #[arg(long, value_enum, default_value_t = McpTransportArg::Stdio)]
        transport: McpTransportArg,
        /// Stdio command
        #[arg(long)]
        command: Option<String>,
        /// Stdio command argument; repeat for multiple args
        #[arg(long = "arg")]
        args: Vec<String>,
        /// HTTP/SSE endpoint URL
        #[arg(long)]
        url: Option<String>,
        /// Environment entry KEY=VALUE; repeat for multiple env vars
        #[arg(long = "env")]
        env: Vec<String>,
        /// Header entry KEY=VALUE; repeat for multiple headers
        #[arg(long = "header")]
        headers: Vec<String>,
        /// Request timeout in milliseconds
        #[arg(long)]
        timeout_ms: Option<u64>,
        /// Mark this server as trusted
        #[arg(long)]
        trust: bool,
    },
    /// Remove a project-local MCP server
    Remove {
        /// Server name
        name: String,
    },
    /// Start octocode as an MCP server, exposing local tools to external agents.
    /// Read-only tools are exposed by default; pass --allow-destructive to also
    /// expose write_file/edit_file/run_command.
    Serve {
        /// Use stdio transport (default; reads JSON-RPC from stdin)
        #[arg(long, default_value_t = true)]
        stdio: bool,
        /// Also expose destructive tools (write/edit/run_command)
        #[arg(long)]
        allow_destructive: bool,
    },
}

#[derive(Subcommand)]
enum McpToolsCommands {
    /// List tools advertised by a configured server
    List {
        /// Server name
        server: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Call one tool on a configured server
    Call {
        /// Server name
        server: String,
        /// Tool name
        tool: String,
        /// Tool arguments as JSON
        #[arg(long, default_value = "{}")]
        arguments: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum McpResourcesCommands {
    /// List resources advertised by a configured server
    List {
        /// Server name
        server: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Read one resource by URI
    Read {
        /// Server name
        server: String,
        /// Resource URI
        uri: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CommandsCatalogCommands {
    /// List built-in slash commands and discovered custom command files
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Filter by name, alias, or description
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// Show the prompt template for one custom command
    Show {
        /// Custom command name, such as /fix-docs
        name: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run a project or user custom command prompt template
    Run {
        /// Custom command name, such as /fix-docs
        name: String,
        /// Render the command without calling the model
        #[arg(long)]
        dry_run: bool,
        /// Print JSON; for real runs this is equivalent to --output-format json
        #[arg(long)]
        json: bool,
        /// Enable thinking mode
        #[arg(short, long)]
        thinking: bool,
        /// Output format for real runs: text, json, stream-json
        #[arg(long, value_enum, default_value_t = OutputFormatArg::Text)]
        output_format: OutputFormatArg,
        /// Arguments passed to the prompt template
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show command search locations
    Locations {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Explain source precedence and effective config values
    Explain {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SettingsCommands {
    /// List editable settings
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Read one setting
    Get {
        /// Setting key, such as ui.theme
        key: String,
    },
    /// Write one project-local setting
    Set {
        /// Setting key, such as ui.theme
        key: String,
        /// Setting value
        value: String,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// List saved sessions for this project
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Replay stored session events without executing tools
    Replay {
        /// Session id, id prefix, name, or latest when omitted
        session: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Export a saved session transcript
    Export {
        /// Session id, id prefix, name, or latest when omitted
        session: Option<String>,
        /// Output format: markdown, json, text
        #[arg(short, long, default_value = "markdown")]
        format: String,
    },
}

#[derive(Subcommand)]
enum MissionCommands {
    /// Create a new dry-run mission record
    New {
        /// Task or mission goal
        task: String,
        /// Create a dry-run mission plan
        #[arg(long)]
        dry_run: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show mission state
    Status {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Inspect mission, plan, and optionally events
    Inspect {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Include events
        #[arg(long)]
        events: bool,
    },
    /// Replay mission events and reconstruct state
    Replay {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// List local mission records
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Filter by lifecycle status
        #[arg(long, value_enum)]
        status: Option<MissionStatusArg>,
        /// Maximum number of missions to print
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Mark a mission as running
    Start {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a mission as paused
    Pause {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Resume a paused mission
    Resume {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a mission as completed
    Complete {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a mission as failed
    Fail {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Failure reason
        #[arg(long, default_value = "manual failure")]
        message: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a mission as cancelled
    Cancel {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Cancellation reason
        #[arg(long, default_value = "manual cancellation")]
        message: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Append a note to the mission replay timeline
    Note {
        /// Mission id, id prefix, or latest
        target: Option<String>,
        /// Note text
        #[arg(long)]
        message: String,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MissionStatusArg {
    Created,
    Planned,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl MissionStatusArg {
    fn status(self) -> crate::mission::MissionStatus {
        match self {
            Self::Created => crate::mission::MissionStatus::Created,
            Self::Planned => crate::mission::MissionStatus::Planned,
            Self::Running => crate::mission::MissionStatus::Running,
            Self::Paused => crate::mission::MissionStatus::Paused,
            Self::Completed => crate::mission::MissionStatus::Completed,
            Self::Failed => crate::mission::MissionStatus::Failed,
            Self::Cancelled => crate::mission::MissionStatus::Cancelled,
        }
    }
}

pub async fn run() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    let launch_bare_tui =
        cli.command.is_none() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    init_tracing(
        matches!(
            cli.command,
            Some(Commands::Tui { .. } | Commands::PreviewTui { .. })
        ) || launch_bare_tui,
    );

    match cli.command {
        Some(Commands::Doctor) => cli::doctor(cli.project_root).await,
        Some(Commands::Login { api_key }) => {
            let root = cli.project_root.or_else(crate::storage::find_project_root);
            cli::login(api_key, root.as_deref()).await
        }
        Some(Commands::Chat {
            prompt,
            thinking,
            model,
            session,
            output_format,
            tool_approval,
            auto_approve,
        }) => {
            let tool_approval =
                effective_turn_tool_approval(output_format, tool_approval, auto_approve);
            cli::chat(
                prompt,
                thinking,
                model,
                cli.project_root,
                session,
                output_format.turn_output_format(),
                tool_approval,
            )
            .await
        }
        Some(Commands::Ask {
            question,
            output_format,
            tool_approval,
            auto_approve,
        }) => {
            let tool_approval =
                effective_turn_tool_approval(output_format, tool_approval, auto_approve);
            cli::ask(
                question,
                cli.project_root,
                output_format.turn_output_format(),
                tool_approval,
            )
            .await
        }
        Some(Commands::Search {
            query,
            code_only,
            limit,
        }) => cli::search(query, code_only, limit, cli.project_root).await,
        Some(Commands::Plan { task }) => cli::plan(task, cli.project_root).await,
        Some(Commands::Run {
            task,
            thinking,
            tool_approval,
            auto_approve,
            output_format,
        }) => {
            let tool_approval =
                effective_turn_tool_approval(output_format, tool_approval, auto_approve);
            cli::run(
                task,
                thinking,
                cli.project_root,
                output_format.turn_output_format(),
                tool_approval,
            )
            .await
        }
        Some(Commands::Resume { session }) => cli::resume(session, cli.project_root).await,
        Some(Commands::Export { session_id, format }) => {
            cli::export(session_id, Some(format), cli.project_root).await
        }
        Some(Commands::Tui {
            thinking,
            model,
            session,
        }) => tui::run_tui(cli.project_root, thinking, model, session).await,
        Some(Commands::PreviewTui {
            width,
            height,
            api,
            scenario,
            theme,
            elapsed_ms,
        }) => {
            let root = crate::cli::resolve_project_root_or_cwd(cli.project_root);
            let scenario = match scenario {
                PreviewScenario::Welcome => tui::app::PreviewSnapshotScenario::Welcome,
                PreviewScenario::Workbench => tui::app::PreviewSnapshotScenario::Workbench,
            };
            let theme = match theme {
                PreviewTheme::Auto => tui::theme::ThemeMode::Auto,
                PreviewTheme::Light => tui::theme::ThemeMode::Light,
                PreviewTheme::Dark => tui::theme::ThemeMode::Dark,
                PreviewTheme::HighContrast => tui::theme::ThemeMode::HighContrast,
            };
            let snapshot = tui::app::render_preview_snapshot(
                root,
                matches!(api, PreviewApiState::Missing),
                width,
                height,
                scenario,
                theme,
                elapsed_ms,
            )?;
            println!("{snapshot}");
            Ok(())
        }
        Some(Commands::Assess { task }) => cli::assess(task, cli.project_root).await,
        Some(Commands::Review {
            parallel,
            max_turns,
            output,
        }) => cli::review(cli.project_root, parallel, max_turns, output).await,
        Some(Commands::Features { command }) => {
            let command = match command {
                FeaturesCommands::Matrix { json } => {
                    cli::features::FeaturesCommand::Matrix { json }
                }
                FeaturesCommands::Status { json } => {
                    cli::features::FeaturesCommand::Status { json }
                }
                FeaturesCommands::Recommend { task, json } => {
                    cli::features::FeaturesCommand::Recommend { task, json }
                }
            };
            cli::features(command, cli.project_root).await
        }
        Some(Commands::Models { json }) => cli::models(json, cli.project_root).await,
        Some(Commands::Goal { command }) => {
            let command = match command {
                GoalCommands::Init { overwrite, json } => {
                    cli::goal::GoalCommand::Init { overwrite, json }
                }
                GoalCommands::Show { json } => cli::goal::GoalCommand::Show { json },
                GoalCommands::Plan { json } => cli::goal::GoalCommand::Plan { json },
                GoalCommands::Graph { json } => cli::goal::GoalCommand::Graph { json },
                GoalCommands::SetActive {
                    id,
                    title,
                    success,
                    targets,
                    json,
                } => cli::goal::GoalCommand::SetActive {
                    id,
                    title,
                    success,
                    targets,
                    json,
                },
                GoalCommands::Run { model, full, json } => {
                    cli::goal::GoalCommand::Run { model, full, json }
                }
            };
            cli::goal(command, cli.project_root).await
        }
        Some(Commands::Archive { command }) => {
            let command = match command {
                ArchiveCommands::Status { json } => cli::archive::ArchiveCommand::Status { json },
                ArchiveCommands::List { json } => cli::archive::ArchiveCommand::List { json },
                ArchiveCommands::Show { candidate_id, json } => {
                    cli::archive::ArchiveCommand::Show { candidate_id, json }
                }
                ArchiveCommands::Lineage { json } => cli::archive::ArchiveCommand::Lineage { json },
            };
            cli::archive(command, cli.project_root).await
        }
        Some(Commands::Repair { command }) => {
            let command = match command {
                RepairCommands::Propose {
                    problem,
                    title,
                    targets,
                    json,
                } => cli::repair::RepairCommand::Propose {
                    problem,
                    title,
                    targets,
                    json,
                },
                RepairCommands::Run {
                    proposal_id,
                    full,
                    model_patch,
                    model,
                    json,
                } => cli::repair::RepairCommand::Run {
                    proposal_id,
                    full,
                    model_patch,
                    model,
                    json,
                },
                RepairCommands::Report { run_id } => cli::repair::RepairCommand::Report { run_id },
                RepairCommands::Status { json } => cli::repair::RepairCommand::Status { json },
            };
            cli::repair(command, cli.project_root).await
        }
        Some(Commands::Knowledge { command }) => {
            let command = match command {
                KnowledgeCommands::Update { json } => {
                    cli::knowledge::KnowledgeCommand::Update { json }
                }
                KnowledgeCommands::Show { topic, json } => {
                    let topic = match topic {
                        KnowledgeTopicArg::Project => cli::knowledge::KnowledgeTopic::Project,
                        KnowledgeTopicArg::RiskMap => cli::knowledge::KnowledgeTopic::RiskMap,
                        KnowledgeTopicArg::Failures => cli::knowledge::KnowledgeTopic::Failures,
                    };
                    cli::knowledge::KnowledgeCommand::Show { topic, json }
                }
            };
            cli::knowledge(command, cli.project_root).await
        }
        Some(Commands::Skill { command }) => {
            let command = match command {
                SkillCommands::List { json } => cli::skill::SkillCommand::List { json },
                SkillCommands::Show { skill_id, json } => {
                    cli::skill::SkillCommand::Show { skill_id, json }
                }
                SkillCommands::Add { run_id, name, json } => {
                    cli::skill::SkillCommand::Add { run_id, name, json }
                }
                SkillCommands::Test { skill_id, json } => {
                    cli::skill::SkillCommand::Test { skill_id, json }
                }
            };
            cli::skill(command, cli.project_root).await
        }
        Some(Commands::Evolve { command }) => {
            let command = match command {
                EvolveCommands::Inspect { json } => {
                    cli::evolution::EvolutionCommand::Inspect { json }
                }
                EvolveCommands::Propose {
                    area,
                    targets,
                    title,
                    problem,
                    json,
                } => cli::evolution::EvolutionCommand::Propose {
                    area,
                    targets,
                    title,
                    problem,
                    json,
                },
                EvolveCommands::Patch {
                    proposal_id,
                    model_agents,
                    model,
                    json,
                } => cli::evolution::EvolutionCommand::Patch {
                    proposal_id,
                    model_agents,
                    model,
                    json,
                },
                EvolveCommands::Repair {
                    proposal_id,
                    model_patch,
                    model,
                    full,
                    json,
                } => cli::evolution::EvolutionCommand::Repair {
                    proposal_id,
                    model_patch,
                    model,
                    full,
                    json,
                },
                EvolveCommands::ProveSelf {
                    task,
                    targets,
                    model,
                    full,
                    json,
                } => cli::evolution::EvolutionCommand::ProveSelf {
                    task,
                    targets,
                    model,
                    full,
                    json,
                },
                EvolveCommands::FromGoal { model, full, json } => {
                    cli::evolution::EvolutionCommand::FromGoal { model, full, json }
                }
                EvolveCommands::Benchmark {
                    rounds,
                    model,
                    full,
                    json,
                } => cli::evolution::EvolutionCommand::Benchmark {
                    rounds,
                    model,
                    full,
                    json,
                },
                EvolveCommands::Memory { json } => {
                    cli::evolution::EvolutionCommand::Memory { json }
                }
                EvolveCommands::Test { run_id, full, json } => {
                    cli::evolution::EvolutionCommand::Test { run_id, full, json }
                }
                EvolveCommands::Apply {
                    run_id,
                    allow_high_risk,
                    json,
                } => cli::evolution::EvolutionCommand::Apply {
                    run_id,
                    allow_high_risk,
                    json,
                },
                EvolveCommands::Rollback { apply_id, json } => {
                    cli::evolution::EvolutionCommand::Rollback { apply_id, json }
                }
                EvolveCommands::Status { json } => {
                    cli::evolution::EvolutionCommand::Status { json }
                }
            };
            cli::evolution(command, cli.project_root).await
        }
        Some(Commands::Agent { command }) => {
            let command = match command {
                AgentCommands::List { json } => cli::agent::AgentCommand::List { json },
                AgentCommands::Show { name, json } => cli::agent::AgentCommand::Show { name, json },
                AgentCommands::Run {
                    name,
                    task,
                    focus,
                    max_turns,
                    model,
                    json,
                } => cli::agent::AgentCommand::Run {
                    name,
                    task,
                    focus,
                    max_turns,
                    model,
                    json,
                },
                AgentCommands::Create { name, template } => cli::agent::AgentCommand::Create {
                    name,
                    template: match template {
                        AgentTemplateArg::Explorer => cli::agent::AgentTemplate::Explorer,
                        AgentTemplateArg::Reviewer => cli::agent::AgentTemplate::Reviewer,
                        AgentTemplateArg::Auditor => cli::agent::AgentTemplate::Auditor,
                        AgentTemplateArg::Tester => cli::agent::AgentTemplate::Tester,
                        AgentTemplateArg::Planner => cli::agent::AgentTemplate::Planner,
                        AgentTemplateArg::Writer => cli::agent::AgentTemplate::Writer,
                    },
                },
                AgentCommands::Validate { name, all, json } => {
                    if all && name.is_some() {
                        return Err(anyhow::anyhow!(
                            "use either an agent name or --all, not both"
                        ));
                    }
                    let target = match name {
                        Some(name) => cli::agent::AgentValidateTarget::One(name),
                        None => {
                            if !all {
                                return Err(anyhow::anyhow!("provide an agent name or pass --all"));
                            }
                            cli::agent::AgentValidateTarget::All
                        }
                    };
                    cli::agent::AgentCommand::Validate { target, json }
                }
            };
            cli::agent(command, cli.project_root).await
        }
        Some(Commands::Mcp { command }) => {
            // `serve` short-circuits: it speaks JSON-RPC on stdio and never
            // returns a value-formatted McpCommand. All other subcommands fall
            // through to the standard cli::mcp dispatch.
            if let McpCommands::Serve {
                stdio: _,
                allow_destructive,
            } = command
            {
                let project_root = cli::resolve_project_root(cli.project_root, "mcp serve")?;
                return crate::mcp::serve_stdio(project_root, allow_destructive).await;
            }
            let command = match command {
                McpCommands::List { json } => cli::mcp::McpCommand::List { json },
                McpCommands::Get { name, json } => cli::mcp::McpCommand::Get { name, json },
                McpCommands::Status { json, connect } => {
                    cli::mcp::McpCommand::Status { json, connect }
                }
                McpCommands::Tools { command } => match command {
                    McpToolsCommands::List { server, json } => {
                        cli::mcp::McpCommand::ToolsList { server, json }
                    }
                    McpToolsCommands::Call {
                        server,
                        tool,
                        arguments,
                        json,
                    } => cli::mcp::McpCommand::ToolsCall {
                        server,
                        tool,
                        arguments,
                        json,
                    },
                },
                McpCommands::Resources { command } => match command {
                    McpResourcesCommands::List { server, json } => {
                        cli::mcp::McpCommand::ResourcesList { server, json }
                    }
                    McpResourcesCommands::Read { server, uri, json } => {
                        cli::mcp::McpCommand::ResourcesRead { server, uri, json }
                    }
                },
                McpCommands::Add {
                    name,
                    transport,
                    command,
                    args,
                    url,
                    env,
                    headers,
                    timeout_ms,
                    trust,
                } => cli::mcp::McpCommand::Add(cli::mcp::McpAddArgs {
                    name,
                    transport: transport.into(),
                    command,
                    args,
                    url,
                    env,
                    headers,
                    timeout_ms,
                    trust,
                }),
                McpCommands::Remove { name } => cli::mcp::McpCommand::Remove { name },
                McpCommands::Serve { .. } => unreachable!("serve handled above"),
            };
            cli::mcp(command, cli.project_root).await
        }
        Some(Commands::Catalog { command }) => {
            let command = match command {
                CommandsCatalogCommands::List { json, filter } => {
                    cli::command_catalog::CommandCatalogCommand::List { json, filter }
                }
                CommandsCatalogCommands::Show { name, json } => {
                    cli::command_catalog::CommandCatalogCommand::Show { name, json }
                }
                CommandsCatalogCommands::Run {
                    name,
                    args,
                    dry_run,
                    json,
                    thinking,
                    output_format,
                } => cli::command_catalog::CommandCatalogCommand::Run {
                    name,
                    args,
                    dry_run,
                    json,
                    thinking,
                    output_format: output_format.turn_output_format(),
                },
                CommandsCatalogCommands::Locations { json } => {
                    cli::command_catalog::CommandCatalogCommand::Locations { json }
                }
            };
            cli::command_catalog(command, cli.project_root).await
        }
        Some(Commands::Config { command }) => {
            let command = match command {
                ConfigCommands::Explain { json } => {
                    cli::config_cmd::ConfigCommand::Explain { json }
                }
            };
            cli::config_cmd(command, cli.project_root).await
        }
        Some(Commands::Settings { command }) => {
            let command = match command {
                SettingsCommands::List { json } => cli::settings::SettingsCommand::List { json },
                SettingsCommands::Get { key } => cli::settings::SettingsCommand::Get { key },
                SettingsCommands::Set { key, value } => {
                    cli::settings::SettingsCommand::Set { key, value }
                }
            };
            cli::settings(command, cli.project_root).await
        }
        Some(Commands::Session { command }) => {
            let command = match command {
                SessionCommands::List { json } => cli::session::SessionCommand::List { json },
                SessionCommands::Replay { session, json } => {
                    cli::session::SessionCommand::Replay { session, json }
                }
                SessionCommands::Export { session, format } => {
                    cli::session::SessionCommand::Export { session, format }
                }
            };
            cli::session(command, cli.project_root).await
        }
        Some(Commands::Mission { command }) => {
            let command = match command {
                MissionCommands::New {
                    task,
                    dry_run,
                    json,
                } => cli::mission::MissionCommand::New {
                    task,
                    dry_run,
                    json,
                },
                MissionCommands::Status { target, json } => {
                    cli::mission::MissionCommand::Status { target, json }
                }
                MissionCommands::Inspect {
                    target,
                    json,
                    events,
                } => cli::mission::MissionCommand::Inspect {
                    target,
                    json,
                    events,
                },
                MissionCommands::Replay { target, json } => {
                    cli::mission::MissionCommand::Replay { target, json }
                }
                MissionCommands::List {
                    json,
                    status,
                    limit,
                } => cli::mission::MissionCommand::List {
                    json,
                    status: status.map(MissionStatusArg::status),
                    limit,
                },
                MissionCommands::Start { target, json } => {
                    cli::mission::MissionCommand::Start { target, json }
                }
                MissionCommands::Pause { target, json } => {
                    cli::mission::MissionCommand::Pause { target, json }
                }
                MissionCommands::Resume { target, json } => {
                    cli::mission::MissionCommand::Resume { target, json }
                }
                MissionCommands::Complete { target, json } => {
                    cli::mission::MissionCommand::Complete { target, json }
                }
                MissionCommands::Fail {
                    target,
                    message,
                    json,
                } => cli::mission::MissionCommand::Fail {
                    target,
                    message,
                    json,
                },
                MissionCommands::Cancel {
                    target,
                    message,
                    json,
                } => cli::mission::MissionCommand::Cancel {
                    target,
                    message,
                    json,
                },
                MissionCommands::Note {
                    target,
                    message,
                    json,
                } => cli::mission::MissionCommand::Note {
                    target,
                    message,
                    json,
                },
            };
            cli::mission(command, cli.project_root).await
        }
        Some(Commands::Task { command }) => {
            let command = match command {
                TaskCommands::List => cli::task::TaskCommand::List,
                TaskCommands::Add { kind, prompt } => cli::task::TaskCommand::Add {
                    kind: kind.parse().map_err(anyhow::Error::msg)?,
                    prompt,
                },
                TaskCommands::Pause { id } => cli::task::TaskCommand::Pause { id },
                TaskCommands::Resume { id } => cli::task::TaskCommand::Resume { id },
                TaskCommands::Run { id, format } => cli::task::TaskCommand::Run {
                    id,
                    format: format.map(OutputFormatArg::turn_output_format),
                },
                TaskCommands::Logs { id } => cli::task::TaskCommand::Logs { id },
                TaskCommands::Rm { id } => cli::task::TaskCommand::Remove { id },
            };
            cli::task(command, cli.project_root).await
        }
        None if launch_bare_tui => tui::run_tui(cli.project_root, false, None, None).await,
        None => cli::welcome(cli.project_root, false, None, None).await,
    }
}

fn init_tracing(quiet_terminal: bool) {
    let default_filter = if quiet_terminal { "error" } else { "info" };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter)),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("invalid positive integer: {error}"))?;
    if parsed == 0 {
        return Err("must be at least 1".to_string());
    }
    Ok(parsed)
}

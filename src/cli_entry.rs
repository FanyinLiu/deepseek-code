use std::{io::IsTerminal, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{cli, tui};

#[derive(Parser)]
#[command(version, about = "DeepSeek-native coding agent", long_about = None)]
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

    /// Store `DeepSeek` API key in system keyring
    Login {
        /// API key (starts with sk-)
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
    },

    /// Ask a question with search context (read-only, no edits)
    Ask {
        /// The question to ask
        question: String,
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

    /// Manage built-in and project custom agents
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
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
    Run { id: String },
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
    },
    /// List local mission records
    List {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
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
        }) => cli::chat(prompt, thinking, model, cli.project_root, session).await,
        Some(Commands::Ask { question }) => cli::ask(question, cli.project_root).await,
        Some(Commands::Search {
            query,
            code_only,
            limit,
        }) => cli::search(query, code_only, limit, cli.project_root).await,
        Some(Commands::Plan { task }) => cli::plan(task, cli.project_root).await,
        Some(Commands::Run { task, thinking }) => cli::run(task, thinking, cli.project_root).await,
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
            let root = cli.project_root.unwrap_or_else(|| {
                crate::storage::find_project_root().unwrap_or_else(|| ".".into())
            });
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
                MissionCommands::Replay { target } => {
                    cli::mission::MissionCommand::Replay { target }
                }
                MissionCommands::List { json } => cli::mission::MissionCommand::List { json },
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
                TaskCommands::Run { id } => cli::task::TaskCommand::Run { id },
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

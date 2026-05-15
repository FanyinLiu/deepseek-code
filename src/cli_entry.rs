use std::path::PathBuf;

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
        #[arg(short, long, default_value = "4")]
        parallel: usize,

        /// Max tool-call turns per reviewer (default: 15)
        #[arg(long, default_value = "15")]
        max_turns: u32,

        /// Save report to file instead of printing
        #[arg(short, long)]
        output: Option<PathBuf>,
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
}

pub async fn run() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    init_tracing(cli.command.as_ref());

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
        None => cli::welcome(cli.project_root, false, None, None).await,
    }
}

fn init_tracing(command: Option<&Commands>) {
    if command_suppresses_terminal_tracing(command) {
        return;
    }

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
        )
        .with_target(false)
        .try_init();
}

fn command_suppresses_terminal_tracing(command: Option<&Commands>) -> bool {
    matches!(
        command,
        Some(Commands::Tui { .. } | Commands::PreviewTui { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_ui_commands_do_not_emit_tracing_into_the_screen() {
        assert!(command_suppresses_terminal_tracing(Some(&Commands::Tui {
            thinking: false,
            model: None,
            session: None,
        })));
        assert!(command_suppresses_terminal_tracing(Some(
            &Commands::PreviewTui {
                width: 120,
                height: 28,
                api: PreviewApiState::Ready,
                scenario: PreviewScenario::Welcome,
                theme: PreviewTheme::Dark,
                elapsed_ms: 0,
            },
        )));
        assert!(!command_suppresses_terminal_tracing(Some(
            &Commands::Doctor
        )));
        assert!(!command_suppresses_terminal_tracing(None));
    }
}

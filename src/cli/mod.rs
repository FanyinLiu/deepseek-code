pub mod agent;
pub mod archive;
pub mod ask;
pub mod assess;
pub mod chat;
pub mod command_catalog;
pub mod config_cmd;
pub mod doctor;
pub mod evolution;
pub mod features;
pub mod goal;
pub mod knowledge;
pub mod login;
pub mod mcp;
pub mod mission;
pub mod models;
pub mod output_blocks;
pub mod plan;
pub mod repair;
pub mod replay;
pub mod resume;
pub mod review;
pub mod root;
pub mod run;
pub mod search;
pub mod session;
pub mod settings;
pub mod skill;
pub mod stream_json;
pub mod task;
pub mod welcome;

pub use agent::agent;
pub use archive::archive;
pub use ask::ask;
pub use assess::assess;
pub use chat::chat;
pub use command_catalog::command_catalog;
pub use config_cmd::config_cmd;
pub use doctor::doctor;
pub use evolution::evolution;
pub use features::features;
pub use goal::goal;
pub use knowledge::knowledge;
pub use login::login;
pub use mcp::mcp;
pub use mission::mission;
pub use models::models;
pub use plan::plan;
pub use repair::repair;
pub use resume::{export, resume};
pub use review::review;
pub use root::{resolve_project_root, resolve_project_root_or_cwd};
pub use run::run;
pub use search::search;
pub use session::session;
pub use settings::settings;
pub use skill::skill;
pub use stream_json::TurnOutputFormat;
pub use task::task;
pub use welcome::welcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalPolicy {
    Ask,
    Allow,
    Deny,
}

impl ToolApprovalPolicy {
    #[must_use]
    pub fn auto_approved(self, auto_approve_session: bool) -> Option<bool> {
        match self {
            Self::Allow => Some(true),
            Self::Deny => Some(false),
            Self::Ask => {
                if auto_approve_session {
                    Some(true)
                } else {
                    None
                }
            }
        }
    }

    #[must_use]
    pub const fn should_deny(self) -> bool {
        matches!(self, Self::Deny)
    }

    #[must_use]
    pub const fn should_allow(self) -> bool {
        matches!(self, Self::Allow)
    }
}

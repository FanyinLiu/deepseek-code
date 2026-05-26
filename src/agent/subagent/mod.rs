pub mod executor;
pub mod registry;
pub mod types;
pub mod worktree;

pub use executor::SubagentExecutor;
pub use registry::SubagentRegistry;
pub use types::*;
pub use worktree::{finalize_worktree, maybe_start_worktree, WorktreeGuard};

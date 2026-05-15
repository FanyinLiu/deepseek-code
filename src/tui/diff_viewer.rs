/// Status of a diff item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Pending,
    Accepted,
    Rejected,
}

/// A single file diff item shown inline in the transcript.
#[derive(Debug, Clone)]
pub struct FileDiffItem {
    pub path: String,
    pub diff: String,
    pub stats: String,
    pub status: DiffStatus,
}

impl FileDiffItem {
    #[must_use]
    pub fn new(path: impl Into<String>, diff: impl Into<String>, stats: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            diff: diff.into(),
            stats: stats.into(),
            status: DiffStatus::Pending,
        }
    }
}

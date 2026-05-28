pub mod atomic;
pub mod cache_index;
pub mod config;
pub mod events;
pub mod input_history;
pub mod keyring;
pub mod missions;
pub mod scheduled_tasks;
pub mod sessions;
pub mod transcripts;
pub mod usage;

use std::path::{Path, PathBuf};

pub use config::{find_project_root, find_project_root_strict, Config};
pub use events::{EventLogStore, SessionEvent, SessionEventKind};
pub use keyring::{
    api_key_env_hint, config_api_key, get_api_key, get_api_key_for_provider,
    get_api_key_without_keyring, get_api_key_without_keyring_for_provider, get_effective_api_key,
    get_env_api_key, get_env_api_key_for_provider, get_keyring_api_key, store_api_key,
    store_api_key_with_project_fallback, ApiKeyStoreLocation,
};
pub use missions::{MissionEventsLoad, MissionStore};
pub use scheduled_tasks::{
    ScheduledTask, ScheduledTaskKind, ScheduledTaskStatus, ScheduledTaskStore,
};
pub use sessions::SessionStore;
pub use transcripts::{export_transcript, TranscriptFormat};
pub use usage::{record_usage_event, UsageEvent, UsageStore, UsageSummary, UsageTotals};

#[must_use]
pub fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("OCTOCODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("DEEPSEEK_CODE_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(dirs::home_dir)
}

#[must_use]
pub fn user_config_dir() -> Option<PathBuf> {
    user_home_dir().map(|home| home.join(".octocode"))
}

pub(crate) fn project_storage_key(project_root: &Path) -> String {
    use sha2::{Digest, Sha256};
    use unicode_normalization::UnicodeNormalization;

    let project_root = config::normalize_project_root(project_root);
    let normalized =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let display = normalized.to_string_lossy().replace('\\', "/");
    let nfc = display.nfc().collect::<String>();
    let digest = Sha256::digest(nfc.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn legacy_project_storage_key(project_root: &Path) -> String {
    use std::hash::Hasher;
    let project_root = config::normalize_project_root(project_root);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&project_root.to_string_lossy().to_string(), &mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(crate) fn project_storage_keys(project_root: &Path) -> Vec<String> {
    let stable = project_storage_key(project_root);
    let legacy = legacy_project_storage_key(project_root);
    if stable == legacy {
        vec![stable]
    } else {
        vec![stable, legacy]
    }
}

pub const TEXT_FILE_SIZE_CAP: u64 = 50 * 1024 * 1024;

pub fn read_text_file_capped(path: impl AsRef<Path>) -> Result<String, anyhow::Error> {
    let path = path.as_ref();
    let size = std::fs::metadata(path)?.len();
    if size > TEXT_FILE_SIZE_CAP {
        anyhow::bail!("file too large: {size} bytes, cap {TEXT_FILE_SIZE_CAP}");
    }
    Ok(std::fs::read_to_string(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_text_file_capped_reads_small_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("small.txt");
        std::fs::write(&path, "hello").expect("write small");

        assert_eq!(read_text_file_capped(&path).expect("read"), "hello");
    }

    #[test]
    fn read_text_file_capped_rejects_large_files_before_reading() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("large.txt");
        let mut file = std::fs::File::create(&path).expect("create large");
        file.write_all(b"x").expect("seed large");
        file.set_len(TEXT_FILE_SIZE_CAP + 1).expect("extend large");

        let error = read_text_file_capped(&path).expect_err("large file should fail");
        assert_eq!(
            error.to_string(),
            format!(
                "file too large: {} bytes, cap {}",
                TEXT_FILE_SIZE_CAP + 1,
                TEXT_FILE_SIZE_CAP
            )
        );
    }
}

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), anyhow::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let temp_path = unique_temp_path(path);
    let write_result = (|| -> Result<(), anyhow::Error> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        replace_path(&temp_path, path)?;
        sync_parent_dir(parent);
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    write_result
}

pub fn write_text_atomic(path: &Path, value: &str) -> Result<(), anyhow::Error> {
    write_atomic(path, value.as_bytes())
}

pub fn write_json_pretty_atomic<T: serde::Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), anyhow::Error> {
    write_text_atomic(path, &serde_json::to_string_pretty(value)?)
}

#[cfg(unix)]
pub fn write_private_text_atomic(path: &Path, value: &str) -> Result<(), anyhow::Error> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let temp_path = unique_temp_path(path);
    let write_result = (|| -> Result<(), anyhow::Error> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp_path)?;
        file.write_all(value.as_bytes())?;
        file.sync_all()?;
        replace_path(&temp_path, path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        sync_parent_dir(parent);
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    write_result
}

#[cfg(not(unix))]
pub fn write_private_text_atomic(path: &Path, value: &str) -> Result<(), anyhow::Error> {
    write_text_atomic(path, value)
}

pub fn append_jsonl_locked(path: &Path, line: &str) -> Result<(), anyhow::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let lock_path = lock_path_for(path);
    let lock_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    fs2::FileExt::lock_exclusive(&lock_file)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    file.sync_all()?;
    Ok(())
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("atomic");
    path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()))
}

fn lock_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("atomic");
    path.with_file_name(format!(".{file_name}.lock"))
}

#[cfg(not(windows))]
fn replace_path(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(windows)]
fn replace_path(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let to = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    let replaced = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_parent_dir(parent: &Path) {
    if let Ok(file) = std::fs::File::open(parent) {
        let _ = file.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent_dir(_parent: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_file_contents() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("value.txt");

        write_text_atomic(&path, "old").expect("write old");
        write_text_atomic(&path, "new").expect("write new");

        assert_eq!(std::fs::read_to_string(path).expect("read"), "new");
    }
}

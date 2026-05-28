use std::path::PathBuf;

/// Lightweight local cache index for search results and context summaries.
/// Avoids re-computing expensive operations across sessions.
pub struct CacheIndex {
    base_path: PathBuf,
}

impl CacheIndex {
    #[must_use]
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Store a cached value under a given key.
    pub fn put(&self, namespace: &str, key: &str, value: &str) -> Result<(), anyhow::Error> {
        let dir = self.base_path.join("cache").join(namespace);
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(sanitize_key(key));
        std::fs::write(&path, value)?;
        Ok(())
    }

    /// Retrieve a cached value, if it exists and is younger than `ttl_seconds`.
    #[must_use]
    pub fn get(&self, namespace: &str, key: &str, ttl_seconds: u64) -> Option<String> {
        let path = self
            .base_path
            .join("cache")
            .join(namespace)
            .join(sanitize_key(key));
        if !path.exists() {
            return None;
        }

        // Check TTL
        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(modified) = meta.modified() {
                let age = modified.elapsed().ok()?;
                if age.as_secs() > ttl_seconds {
                    // Stale — delete and return None
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
            }
        }

        crate::storage::read_text_file_capped(&path).ok()
    }

    /// Invalidate a specific cache entry.
    pub fn invalidate(&self, namespace: &str, key: &str) {
        let path = self
            .base_path
            .join("cache")
            .join(namespace)
            .join(sanitize_key(key));
        let _ = std::fs::remove_file(&path);
    }

    /// Invalidate an entire namespace.
    pub fn invalidate_namespace(&self, namespace: &str) {
        let dir = self.base_path.join("cache").join(namespace);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
}

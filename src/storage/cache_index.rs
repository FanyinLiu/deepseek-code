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
        let dir = self.cache_namespace_dir(namespace);
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(sanitize_component(key));
        crate::storage::atomic::write_text_atomic(&path, value)?;
        Ok(())
    }

    /// Retrieve a cached value, if it exists and is younger than `ttl_seconds`.
    #[must_use]
    pub fn get(&self, namespace: &str, key: &str, ttl_seconds: u64) -> Option<String> {
        let path = self
            .cache_namespace_dir(namespace)
            .join(sanitize_component(key));
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
            .cache_namespace_dir(namespace)
            .join(sanitize_component(key));
        let _ = std::fs::remove_file(&path);
    }

    /// Invalidate an entire namespace.
    pub fn invalidate_namespace(&self, namespace: &str) {
        let dir = self.cache_namespace_dir(namespace);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn cache_namespace_dir(&self, namespace: &str) -> PathBuf {
        self.base_path
            .join("cache")
            .join(sanitize_component(namespace))
    }
}

fn sanitize_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "_".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_cannot_escape_cache_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = CacheIndex::new(root.path().join("state"));
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&outside).expect("create outside");
        std::fs::write(outside.join("keep.txt"), "keep").expect("write outside");

        store
            .put("../outside", "../key", "cached")
            .expect("put cache entry");
        store.invalidate_namespace("../outside");

        assert_eq!(
            std::fs::read_to_string(outside.join("keep.txt")).expect("outside survives"),
            "keep"
        );
        assert!(store.get("../outside", "../key", 60).is_none());
    }

    #[test]
    fn sanitized_namespace_roundtrips() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = CacheIndex::new(root.path().join("state"));

        store
            .put("search/results", "foo/bar", "value")
            .expect("put");

        assert_eq!(
            store.get("search/results", "foo/bar", 60).as_deref(),
            Some("value")
        );
    }
}

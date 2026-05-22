use std::collections::BTreeSet;

use crate::deepseek::ToolDefinition;

use super::metadata::{self, ToolMetadata};

#[derive(Debug, Clone)]
pub struct ToolRegistryEntry {
    pub metadata: &'static ToolMetadata,
    pub definition: ToolDefinition,
    pub dispatch_supported: bool,
    pub mcp_exposed_by_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolRegistryError {
    #[error("duplicate tool metadata entry: {0}")]
    DuplicateMetadata(String),
    #[error("duplicate model tool schema: {0}")]
    DuplicateSchema(String),
    #[error("model tool schema missing metadata: {0}")]
    SchemaMissingMetadata(String),
    #[error("dispatch tool missing metadata: {0}")]
    DispatchMissingMetadata(String),
    #[error("dispatch tool missing model schema: {0}")]
    DispatchMissingSchema(String),
}

#[must_use]
pub fn all() -> Vec<ToolRegistryEntry> {
    crate::deepseek::tools::standard_tool_definitions()
        .into_iter()
        .filter_map(|definition| {
            let name = definition.function.name.clone();
            metadata::metadata(&name).map(|metadata| ToolRegistryEntry {
                metadata,
                definition,
                dispatch_supported: dispatch_supports(&name),
                mcp_exposed_by_default: metadata.read_only,
            })
        })
        .collect()
}

#[must_use]
pub fn get(name: &str) -> Option<ToolRegistryEntry> {
    all()
        .into_iter()
        .find(|entry| entry.metadata.name == name || entry.definition.function.name == name)
}

#[must_use]
pub fn dispatch_supports(name: &str) -> bool {
    crate::tools::dispatch::SUPPORTED_TOOL_NAMES.contains(&name)
}

pub fn assert_consistent() -> Result<(), ToolRegistryError> {
    let mut metadata_names = BTreeSet::new();
    for meta in metadata::ALL_TOOLS {
        if !metadata_names.insert(meta.name) {
            return Err(ToolRegistryError::DuplicateMetadata(meta.name.to_string()));
        }
    }

    let definitions = crate::deepseek::tools::standard_tool_definitions();
    let mut schema_names = BTreeSet::new();
    for definition in &definitions {
        let name = definition.function.name.as_str();
        if !schema_names.insert(name) {
            return Err(ToolRegistryError::DuplicateSchema(name.to_string()));
        }
        if !metadata_names.contains(name) {
            return Err(ToolRegistryError::SchemaMissingMetadata(name.to_string()));
        }
    }

    for name in crate::tools::dispatch::SUPPORTED_TOOL_NAMES {
        if !metadata_names.contains(name) {
            return Err(ToolRegistryError::DispatchMissingMetadata(
                (*name).to_string(),
            ));
        }
        if !schema_names.contains(name) {
            return Err(ToolRegistryError::DispatchMissingSchema(
                (*name).to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent_across_schema_metadata_and_dispatch() {
        assert_consistent().expect("tool registry should be consistent");
    }

    #[test]
    fn known_drift_points_are_registered() {
        for name in ["fetch_url", "web_search", "github_pr", "run_subagent"] {
            let entry = get(name).unwrap_or_else(|| panic!("missing registry entry for {name}"));
            assert_eq!(entry.metadata.name, name);
        }
        assert!(!get("fetch_url").expect("fetch_url").metadata.read_only);
        assert!(!get("github_pr").expect("github_pr").metadata.read_only);
        assert!(
            !get("run_subagent")
                .expect("run_subagent")
                .dispatch_supported
        );
    }
}

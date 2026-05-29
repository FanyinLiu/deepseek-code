//! Lightweight project recognition.
//!
//! Detects the workspace's technology stack from well-known marker files so the
//! agent uses project-native build/test/lint commands instead of guessing.
//! Where a manifest encodes the real commands (npm/composer `scripts`,
//! `Makefile` targets, `pyproject.toml` tooling) those are parsed
//! deterministically — no model call — so we never claim a command the project
//! doesn't actually define. The profile is injected into the system prompt and
//! decomposition prompts as stable context.

use std::collections::HashSet;
use std::path::Path;

/// A detected technology, with the commands that conventionally build/test/lint
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedStack {
    pub language: &'static str,
    pub marker: &'static str,
    pub build: Option<String>,
    pub test: Option<String>,
    pub lint: Option<String>,
}

/// The recognized profile of a workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectProfile {
    pub stacks: Vec<DetectedStack>,
}

/// (language, marker, default build, default test, default lint). Ordered by
/// specificity so the preferred marker for a language wins on dedupe. Defaults
/// are refined from the manifest where one encodes the real commands.
#[allow(clippy::type_complexity)]
const DETECTION_RULES: &[(&str, &str, Option<&str>, Option<&str>, Option<&str>)] = &[
    (
        "Rust",
        "Cargo.toml",
        Some("cargo build"),
        Some("cargo test"),
        Some("cargo clippy"),
    ),
    (
        "Node.js / TypeScript",
        "package.json",
        Some("npm run build"),
        Some("npm test"),
        None,
    ),
    (
        "Go",
        "go.mod",
        Some("go build ./..."),
        Some("go test ./..."),
        Some("go vet ./..."),
    ),
    ("Python", "pyproject.toml", None, Some("pytest"), None),
    ("Python", "requirements.txt", None, Some("pytest"), None),
    (
        "Java (Maven)",
        "pom.xml",
        Some("mvn compile"),
        Some("mvn test"),
        None,
    ),
    (
        "Java/Kotlin (Gradle)",
        "build.gradle",
        Some("gradle build"),
        Some("gradle test"),
        None,
    ),
    ("Ruby", "Gemfile", None, Some("bundle exec rspec"), None),
    ("PHP", "composer.json", None, None, None),
    ("Make", "Makefile", Some("make"), Some("make test"), None),
];

impl ProjectProfile {
    /// Detect the stack(s) present at `project_root` from marker files,
    /// refining commands from the manifest where possible.
    #[must_use]
    pub fn detect(project_root: &Path) -> Self {
        let mut stacks = Vec::new();
        let mut seen_languages = HashSet::new();
        for (language, marker, build, test, lint) in DETECTION_RULES {
            if !project_root.join(marker).exists() || !seen_languages.insert(*language) {
                continue;
            }
            let mut stack = DetectedStack {
                language,
                marker,
                build: build.map(str::to_string),
                test: test.map(str::to_string),
                lint: lint.map(str::to_string),
            };
            match *marker {
                "package.json" => refine_node(project_root, &mut stack),
                "composer.json" => refine_php(project_root, &mut stack),
                "pyproject.toml" => refine_python(project_root, &mut stack),
                "Makefile" => refine_make(project_root, &mut stack),
                _ => {}
            }
            stacks.push(stack);
        }
        Self { stacks }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stacks.is_empty()
    }

    /// The project-native test commands detected, in detection order.
    #[must_use]
    pub fn test_commands(&self) -> Vec<&str> {
        self.stacks
            .iter()
            .filter_map(|stack| stack.test.as_deref())
            .collect()
    }

    /// A one-line hint naming the project-native test commands, for injection
    /// into a test subagent's context. `None` when nothing was recognized.
    #[must_use]
    pub fn test_command_hint(&self) -> Option<String> {
        let commands = self.test_commands();
        if commands.is_empty() {
            return None;
        }
        let list = commands
            .iter()
            .map(|cmd| format!("`{cmd}`"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "Project-native test command(s), prefer these: {list}"
        ))
    }

    /// Concise markdown describing the project, for system-prompt injection.
    /// Returns `None` when nothing was recognized.
    #[must_use]
    pub fn render(&self) -> Option<String> {
        if self.stacks.is_empty() {
            return None;
        }
        let mut out = String::from("## Detected Project\n\n");
        for stack in &self.stacks {
            out.push_str(&format!("- {} (from `{}`)", stack.language, stack.marker));
            if let Some(test) = &stack.test {
                out.push_str(&format!("; tests: `{test}`"));
            }
            if let Some(build) = &stack.build {
                out.push_str(&format!("; build: `{build}`"));
            }
            if let Some(lint) = &stack.lint {
                out.push_str(&format!("; lint: `{lint}`"));
            }
            out.push('\n');
        }
        out.push_str(
            "\nPrefer these project-native commands; do not assume a different toolchain.\n",
        );
        Some(out)
    }
}

/// Refine a Node stack from `package.json` scripts + the lockfile's package
/// manager. Only claims commands the project actually defines.
fn refine_node(project_root: &Path, stack: &mut DetectedStack) {
    let pm = node_package_manager(project_root);
    let scripts = json_script_names(project_root, "package.json");
    stack.test = scripts.contains("test").then(|| format!("{pm} test"));
    stack.build = scripts.contains("build").then(|| format!("{pm} run build"));
    stack.lint = scripts.contains("lint").then(|| format!("{pm} run lint"));
}

fn node_package_manager(project_root: &Path) -> &'static str {
    if project_root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if project_root.join("yarn.lock").exists() {
        "yarn"
    } else if project_root.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    }
}

/// Refine a PHP stack from `composer.json` scripts. Composer runs any named
/// script as `composer <name>`.
fn refine_php(project_root: &Path, stack: &mut DetectedStack) {
    let scripts = json_script_names(project_root, "composer.json");
    stack.test = scripts
        .contains("test")
        .then(|| "composer test".to_string());
    stack.build = scripts
        .contains("build")
        .then(|| "composer build".to_string());
    stack.lint = scripts
        .contains("lint")
        .then(|| "composer lint".to_string());
}

/// Read the keys of the top-level `scripts` object from a JSON manifest.
fn json_script_names(project_root: &Path, manifest: &str) -> HashSet<String> {
    let Ok(content) = crate::storage::read_text_file_capped(project_root.join(manifest)) else {
        return HashSet::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return HashSet::new();
    };
    value
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .map(|scripts| scripts.keys().cloned().collect())
        .unwrap_or_default()
}

/// Refine a Python stack from `pyproject.toml`: prefix the detected runner
/// (poetry/pdm/uv) and surface ruff as the lint command when configured.
fn refine_python(project_root: &Path, stack: &mut DetectedStack) {
    let Ok(content) = crate::storage::read_text_file_capped(project_root.join("pyproject.toml"))
    else {
        return;
    };
    let runner_prefix = if content.contains("[tool.poetry") {
        "poetry run "
    } else if content.contains("[tool.pdm") {
        "pdm run "
    } else if content.contains("[tool.uv") {
        "uv run "
    } else {
        ""
    };
    stack.test = Some(format!("{runner_prefix}pytest"));
    if content.contains("[tool.ruff") {
        stack.lint = Some(format!("{runner_prefix}ruff check ."));
    }
}

/// Refine a Make stack: only claim `make <target>` for declared targets.
fn refine_make(project_root: &Path, stack: &mut DetectedStack) {
    let targets = make_targets(project_root);
    stack.test = targets.contains("test").then(|| "make test".to_string());
    stack.build = if targets.contains("build") {
        Some("make build".to_string())
    } else if targets.contains("all") {
        Some("make".to_string())
    } else {
        None
    };
    stack.lint = targets.contains("lint").then(|| "make lint".to_string());
}

fn make_targets(project_root: &Path) -> HashSet<String> {
    let Ok(content) = crate::storage::read_text_file_capped(project_root.join("Makefile")) else {
        return HashSet::new();
    };
    let mut targets = HashSet::new();
    for line in content.lines() {
        // A target declaration starts at column 0 (recipe lines are indented)
        // and names come before the first `:`.
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        if let Some((name, _)) = line.split_once(':') {
            let name = name.trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            {
                targets.insert(name.to_string());
            }
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).expect("write marker");
    }

    #[test]
    fn detects_rust_with_lint() {
        let temp = tempfile::tempdir().unwrap();
        touch(temp.path(), "Cargo.toml", "");
        let profile = ProjectProfile::detect(temp.path());
        assert_eq!(profile.stacks[0].test.as_deref(), Some("cargo test"));
        assert_eq!(profile.stacks[0].lint.as_deref(), Some("cargo clippy"));
    }

    #[test]
    fn python_is_not_double_counted() {
        let temp = tempfile::tempdir().unwrap();
        touch(temp.path(), "pyproject.toml", "");
        touch(temp.path(), "requirements.txt", "");
        let profile = ProjectProfile::detect(temp.path());
        assert_eq!(
            profile
                .stacks
                .iter()
                .filter(|s| s.language == "Python")
                .count(),
            1
        );
    }

    #[test]
    fn node_reads_scripts_package_manager_and_lint() {
        let temp = tempfile::tempdir().unwrap();
        touch(
            temp.path(),
            "package.json",
            r#"{"scripts":{"test":"vitest","build":"tsc","lint":"eslint ."}}"#,
        );
        touch(temp.path(), "pnpm-lock.yaml", "");
        let node = &ProjectProfile::detect(temp.path()).stacks[0];
        assert_eq!(node.test.as_deref(), Some("pnpm test"));
        assert_eq!(node.build.as_deref(), Some("pnpm run build"));
        assert_eq!(node.lint.as_deref(), Some("pnpm run lint"));
    }

    #[test]
    fn node_without_scripts_claims_nothing() {
        let temp = tempfile::tempdir().unwrap();
        touch(temp.path(), "package.json", r#"{"name":"x"}"#);
        let node = &ProjectProfile::detect(temp.path()).stacks[0];
        assert_eq!(node.test, None);
        assert_eq!(node.lint, None);
    }

    #[test]
    fn php_reads_composer_scripts() {
        let temp = tempfile::tempdir().unwrap();
        touch(
            temp.path(),
            "composer.json",
            r#"{"scripts":{"test":"phpunit","lint":"phpcs"}}"#,
        );
        let php = &ProjectProfile::detect(temp.path()).stacks[0];
        assert_eq!(php.language, "PHP");
        assert_eq!(php.test.as_deref(), Some("composer test"));
        assert_eq!(php.lint.as_deref(), Some("composer lint"));
    }

    #[test]
    fn python_poetry_and_ruff_are_detected() {
        let temp = tempfile::tempdir().unwrap();
        touch(
            temp.path(),
            "pyproject.toml",
            "[tool.poetry]\nname=\"x\"\n[tool.ruff]\nline-length=100\n",
        );
        let py = &ProjectProfile::detect(temp.path()).stacks[0];
        assert_eq!(py.test.as_deref(), Some("poetry run pytest"));
        assert_eq!(py.lint.as_deref(), Some("poetry run ruff check ."));
    }

    #[test]
    fn python_plain_pyproject_uses_bare_pytest() {
        let temp = tempfile::tempdir().unwrap();
        touch(temp.path(), "pyproject.toml", "[project]\nname=\"x\"\n");
        let py = &ProjectProfile::detect(temp.path()).stacks[0];
        assert_eq!(py.test.as_deref(), Some("pytest"));
        assert_eq!(py.lint, None);
    }

    #[test]
    fn make_only_claims_declared_targets() {
        let temp = tempfile::tempdir().unwrap();
        touch(
            temp.path(),
            "Makefile",
            "build:\n\tcargo build\nlint:\n\tclippy\n",
        );
        let make = &ProjectProfile::detect(temp.path()).stacks[0];
        assert_eq!(make.build.as_deref(), Some("make build"));
        assert_eq!(make.lint.as_deref(), Some("make lint"));
        assert_eq!(make.test, None);
    }

    #[test]
    fn render_includes_lint_and_empty_yields_none() {
        let temp = tempfile::tempdir().unwrap();
        touch(temp.path(), "Cargo.toml", "");
        assert!(ProjectProfile::detect(temp.path())
            .render()
            .unwrap()
            .contains("lint: `cargo clippy`"));

        let empty = tempfile::tempdir().unwrap();
        assert!(ProjectProfile::detect(empty.path()).render().is_none());
    }
}

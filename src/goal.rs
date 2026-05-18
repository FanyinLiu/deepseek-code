use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const GOALS_PATH: &str = ".octocode/goals.toml";
pub const GOAL_TASKS_DIR: &str = ".octocode/evolution/tasks";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalConfig {
    pub identity: GoalIdentity,
    pub current_phase: GoalPhase,
    pub active_objective: GoalObjective,
    #[serde(default)]
    pub execution: GoalExecution,
    pub capabilities: BTreeMap<String, String>,
    pub constraints: GoalConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalIdentity {
    pub name: String,
    pub north_star: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalPhase {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalObjective {
    pub id: String,
    pub title: String,
    pub success: Vec<String>,
    #[serde(default)]
    pub target_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalExecution {
    pub model: String,
    pub full_validation: bool,
}

impl Default for GoalExecution {
    fn default() -> Self {
        Self {
            model: "deepseek-v4-flash".to_string(),
            full_validation: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalConstraints {
    pub no_direct_main_mutation: bool,
    pub cloud_sandbox_required: bool,
    pub no_secret_exposure: bool,
    pub no_unverified_core_changes: bool,
    pub one_primary_objective_per_evolution: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalAlignment {
    pub objective_id: String,
    pub objective_title: String,
    pub phase_id: String,
    pub north_star: String,
    pub success_criteria: Vec<String>,
    pub target_files: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalTaskGraph {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub objective_id: String,
    pub objective_title: String,
    pub phase_id: String,
    pub status: GoalTaskGraphStatus,
    pub tasks: Vec<GoalTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskGraphStatus {
    Planned,
    Running,
    Proven,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalTask {
    pub id: String,
    pub title: String,
    pub rationale: String,
    pub status: GoalTaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskStatus {
    Pending,
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct GoalStore {
    project_root: PathBuf,
    path: PathBuf,
}

impl GoalStore {
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        Self {
            path: project_root.join(GOALS_PATH),
            project_root,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init_default(&self, overwrite: bool) -> Result<GoalConfig> {
        if self.path.exists() && !overwrite {
            return self.load_or_default();
        }
        let config = GoalConfig::default();
        self.save(&config)?;
        Ok(config)
    }

    pub fn load_or_default(&self) -> Result<GoalConfig> {
        if !self.path.exists() {
            return Ok(GoalConfig::default());
        }
        let data = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        toml::from_str(&data).with_context(|| format!("invalid {}", self.path.display()))
    }

    pub fn save(&self, config: &GoalConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let data = toml::to_string_pretty(config)?;
        fs::write(&self.path, data)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }

    pub fn set_active(
        &self,
        id: String,
        title: String,
        success: Vec<String>,
        target_files: Vec<String>,
    ) -> Result<GoalConfig> {
        let mut config = self.load_or_default()?;
        let normalized_targets = normalize_targets(target_files);
        config.active_objective = GoalObjective {
            id,
            title: title.clone(),
            success: if success.is_empty() {
                vec![format!(
                    "Octocode can demonstrate measurable progress on {title}"
                )]
            } else {
                success
            },
            target_files: if normalized_targets.is_empty() {
                config.active_objective.target_files
            } else {
                normalized_targets
            },
        };
        self.save(&config)?;
        Ok(config)
    }

    pub fn create_task_graph(&self) -> Result<GoalTaskGraph> {
        let config = self.load_or_default()?;
        let graph = GoalTaskGraph {
            id: format!("taskgraph-{}", Uuid::new_v4()),
            created_at: Utc::now(),
            objective_id: config.active_objective.id.clone(),
            objective_title: config.active_objective.title.clone(),
            phase_id: config.current_phase.id.clone(),
            status: GoalTaskGraphStatus::Planned,
            tasks: task_graph_for_goal(&config),
        };
        self.write_task_graph(&graph)?;
        Ok(graph)
    }

    pub fn complete_task_graph(
        &self,
        mut graph: GoalTaskGraph,
        proven: bool,
    ) -> Result<GoalTaskGraph> {
        graph.status = if proven {
            GoalTaskGraphStatus::Proven
        } else {
            GoalTaskGraphStatus::Failed
        };
        for task in &mut graph.tasks {
            task.status = if proven {
                GoalTaskStatus::Passed
            } else {
                GoalTaskStatus::Failed
            };
        }
        self.write_task_graph(&graph)?;
        Ok(graph)
    }

    pub fn list_task_graphs(&self) -> Result<Vec<GoalTaskGraph>> {
        let dir = self.tasks_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut graphs: Vec<GoalTaskGraph> = Vec::new();
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let data = fs::read_to_string(&path)
                .with_context(|| format!("failed to read task graph {}", path.display()))?;
            graphs.push(
                serde_json::from_str(&data)
                    .with_context(|| format!("invalid task graph {}", path.display()))?,
            );
        }
        graphs.sort_by_key(|graph| graph.created_at);
        Ok(graphs)
    }

    fn tasks_dir(&self) -> PathBuf {
        self.project_root.join(GOAL_TASKS_DIR)
    }

    fn write_task_graph(&self, graph: &GoalTaskGraph) -> Result<()> {
        let dir = self.tasks_dir();
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let path = dir.join(format!("{}.json", graph.id));
        let data = serde_json::to_string_pretty(graph)?;
        fs::write(&path, format!("{data}\n"))
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

impl GoalConfig {
    pub fn alignment(&self) -> GoalAlignment {
        let target_files = self.target_files();
        GoalAlignment {
            objective_id: self.active_objective.id.clone(),
            objective_title: self.active_objective.title.clone(),
            phase_id: self.current_phase.id.clone(),
            north_star: self.identity.north_star.clone(),
            success_criteria: self.active_objective.success.clone(),
            target_files: target_files.clone(),
            summary: format!(
                "Advance `{}` in phase `{}` while preserving Octocode's north-star goal.",
                self.active_objective.id, self.current_phase.id
            ),
        }
    }

    pub fn evolution_task(&self) -> String {
        format!(
            "Improve Octocode toward active objective `{}`: {}. Success criteria: {}",
            self.active_objective.id,
            self.active_objective.title,
            self.active_objective.success.join("; ")
        )
    }

    pub fn target_files(&self) -> Vec<String> {
        let targets = normalize_targets(self.active_objective.target_files.clone());
        if targets.is_empty() {
            vec!["docs/evolution/self-evolution-proof.md".to_string()]
        } else {
            targets
        }
    }

    pub fn execution_model(&self) -> Option<&str> {
        let model = self.execution.model.trim();
        if model.is_empty() {
            None
        } else {
            Some(model)
        }
    }
}

impl Default for GoalConfig {
    fn default() -> Self {
        let capabilities = BTreeMap::from([
            ("model_routing".to_string(), "increase".to_string()),
            ("task_planning".to_string(), "increase".to_string()),
            (
                "multi_agent_coordination".to_string(),
                "increase".to_string(),
            ),
            ("verification_strength".to_string(), "increase".to_string()),
            ("user_control".to_string(), "increase".to_string()),
            ("token_cost".to_string(), "decrease".to_string()),
            ("regression_risk".to_string(), "decrease".to_string()),
        ]);
        Self {
            identity: GoalIdentity {
                name: "Octocode".to_string(),
                north_star: "Become a verified, controllable, low-cost, multi-model, multi-agent coding CLI that improves real-world engineering task success.".to_string(),
            },
            current_phase: GoalPhase {
                id: "phase_1_goal_system".to_string(),
                name: "Goal system and task graph".to_string(),
                description: "Give Octocode persistent goals, task decomposition, and proof-linked evolution records.".to_string(),
            },
            active_objective: GoalObjective {
                id: "goal_binding".to_string(),
                title: "Bind every self-evolution patch to an explicit goal".to_string(),
                success: vec![
                    "Octocode can read .octocode/goals.toml".to_string(),
                    "Self-evolution proof records include goal_alignment".to_string(),
                    "Goal-driven evolution can run from the active objective without relying on chat context".to_string(),
                ],
                target_files: vec!["docs/evolution/self-evolution-proof.md".to_string()],
            },
            execution: GoalExecution {
                model: "deepseek-v4-flash".to_string(),
                full_validation: false,
            },
            capabilities,
            constraints: GoalConstraints {
                no_direct_main_mutation: true,
                cloud_sandbox_required: true,
                no_secret_exposure: true,
                no_unverified_core_changes: true,
                one_primary_objective_per_evolution: true,
            },
        }
    }
}

fn normalize_targets(targets: Vec<String>) -> Vec<String> {
    let mut targets = targets
        .into_iter()
        .map(|path| path.trim().trim_start_matches("./").to_string())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    targets
}

fn task_graph_for_goal(config: &GoalConfig) -> Vec<GoalTask> {
    let mut tasks = vec![
        GoalTask {
            id: "read_active_goal".to_string(),
            title: "Read active goal configuration".to_string(),
            rationale: "Self-evolution must be grounded in .octocode/goals.toml instead of chat context.".to_string(),
            status: GoalTaskStatus::Pending,
        },
        GoalTask {
            id: "build_goal_alignment".to_string(),
            title: "Build goal alignment evidence".to_string(),
            rationale: "Each candidate needs explicit objective, phase, target, and success-criteria evidence.".to_string(),
            status: GoalTaskStatus::Pending,
        },
    ];
    tasks.extend(
        config
            .active_objective
            .success
            .iter()
            .enumerate()
            .map(|(index, criterion)| GoalTask {
                id: format!("success_{}", index + 1),
                title: criterion.clone(),
                rationale: "A success criterion from the active objective must be represented in the proof.".to_string(),
                status: GoalTaskStatus::Pending,
            }),
    );
    tasks.extend([
        GoalTask {
            id: "generate_candidate_patch".to_string(),
            title: "Generate a candidate patch".to_string(),
            rationale: "The model must produce a concrete code or artifact change before the sandbox can verify anything.".to_string(),
            status: GoalTaskStatus::Pending,
        },
        GoalTask {
            id: "remote_sandbox_validation".to_string(),
            title: "Validate candidate in a remote sandbox".to_string(),
            rationale: "The candidate must pass an external validation environment before proof can be trusted.".to_string(),
            status: GoalTaskStatus::Pending,
        },
        GoalTask {
            id: "archive_proof".to_string(),
            title: "Archive proof and evolution memory".to_string(),
            rationale: "Successful and failed attempts should become durable evidence for later evolution.".to_string(),
            status: GoalTaskStatus::Pending,
        },
    ]);
    tasks
}

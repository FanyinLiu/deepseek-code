use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;

use crate::mission::events::{MissionEvent, MissionEventKind};
use crate::mission::state::{completed_state, generate_dry_run_plan, replay_state_from_events};
use crate::mission::types::{Mission, MissionBundle, MissionPlan, MissionState, MissionSummary};

#[derive(Debug, Clone)]
pub struct MissionStore {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MissionEventsLoad {
    pub events: Vec<MissionEvent>,
    pub skipped_malformed_lines: usize,
}

impl MissionStore {
    #[must_use]
    pub fn for_project(project_root: &Path) -> Self {
        Self {
            root: project_root.join(".deepseek-code").join("missions"),
        }
    }

    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn mission_dir(&self, mission_id: &str) -> PathBuf {
        self.root.join(mission_id)
    }

    pub fn create_dry_run(
        &self,
        goal: String,
        project_root: PathBuf,
    ) -> Result<MissionBundle, anyhow::Error> {
        let plan = generate_dry_run_plan(&goal);
        let mut mission = Mission::new_dry_run(goal, plan.recommended_mode.clone(), project_root);
        let created = MissionState::created(&mission);
        let completed = completed_state(&mission.id, plan.steps.len());
        mission.updated_at = completed.updated_at;

        let events = vec![
            MissionEvent::new(
                mission.id.clone(),
                MissionEventKind::MissionCreated {
                    state: created.clone(),
                },
            ),
            MissionEvent::new(
                mission.id.clone(),
                MissionEventKind::PlanGenerated { plan: plan.clone() },
            ),
            MissionEvent::new(
                mission.id.clone(),
                MissionEventKind::MissionCompleted {
                    state: completed.clone(),
                },
            ),
        ];

        let bundle = MissionBundle {
            mission,
            state: completed,
            plan,
            events,
        };
        self.save_bundle(&bundle)?;
        Ok(bundle)
    }

    pub fn save_bundle(&self, bundle: &MissionBundle) -> Result<(), anyhow::Error> {
        let dir = self.mission_dir(&bundle.mission.id);
        std::fs::create_dir_all(&dir)?;
        write_pretty_json(&dir.join("mission.json"), &bundle.mission)?;
        write_pretty_json(&dir.join("state.json"), &bundle.state)?;
        write_pretty_json(&dir.join("plan.json"), &bundle.plan)?;
        std::fs::write(dir.join("events.jsonl"), "")?;
        for event in &bundle.events {
            self.append_event(&bundle.mission.id, event)?;
        }
        self.update_index(&bundle.mission, &bundle.state)?;
        Ok(())
    }

    pub fn append_event(
        &self,
        mission_id: &str,
        event: &MissionEvent,
    ) -> Result<(), anyhow::Error> {
        let path = self.mission_dir(mission_id).join("events.jsonl");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(event)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        use std::io::Write;
        writeln!(file, "{line}")?;
        Ok(())
    }

    pub fn load_bundle(
        &self,
        id_or_latest: &str,
        include_events: bool,
    ) -> Result<MissionBundle, anyhow::Error> {
        let id = self.resolve_id(id_or_latest)?;
        let dir = self.mission_dir(&id);
        let mission: Mission = read_json(&dir.join("mission.json"))?;
        let state: MissionState = read_json(&dir.join("state.json"))?;
        let plan: MissionPlan = read_json(&dir.join("plan.json"))?;
        let events = if include_events {
            self.load_events_lossy(&id)?.events
        } else {
            Vec::new()
        };
        Ok(MissionBundle {
            mission,
            state,
            plan,
            events,
        })
    }

    pub fn load_state(&self, id_or_latest: &str) -> Result<MissionState, anyhow::Error> {
        let id = self.resolve_id(id_or_latest)?;
        read_json(&self.mission_dir(&id).join("state.json"))
    }

    pub fn list(&self) -> Result<Vec<MissionSummary>, anyhow::Error> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let mut summaries: Vec<MissionSummary> = serde_json::from_str(&content)?;
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
        Ok(summaries)
    }

    pub fn latest(&self) -> Result<MissionSummary, anyhow::Error> {
        self.list()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no missions found"))
    }

    pub fn resolve_id(&self, id_or_latest: &str) -> Result<String, anyhow::Error> {
        if id_or_latest == "latest" {
            return Ok(self.latest()?.id);
        }
        let matches: Vec<_> = self
            .list()?
            .into_iter()
            .filter(|summary| summary.id.starts_with(id_or_latest))
            .collect();
        match matches.as_slice() {
            [summary] => Ok(summary.id.clone()),
            [] => anyhow::bail!("mission not found: {id_or_latest}"),
            _ => anyhow::bail!("mission prefix is ambiguous: {id_or_latest}"),
        }
    }

    pub fn load_events_lossy(
        &self,
        id_or_latest: &str,
    ) -> Result<MissionEventsLoad, anyhow::Error> {
        let id = self.resolve_id(id_or_latest)?;
        let path = self.mission_dir(&id).join("events.jsonl");
        if !path.exists() {
            return Ok(MissionEventsLoad {
                events: Vec::new(),
                skipped_malformed_lines: 0,
            });
        }
        let content = std::fs::read_to_string(&path)?;
        let non_empty_lines: Vec<_> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let mut events = Vec::new();
        let mut skipped = 0;
        for (index, line) in non_empty_lines.iter().enumerate() {
            match serde_json::from_str::<MissionEvent>(line) {
                Ok(event) => events.push(event),
                Err(error) if index + 1 == non_empty_lines.len() => {
                    tracing::warn!("ignored malformed final mission event line: {error}");
                    skipped += 1;
                }
                Err(error) => return Err(error).context("parse mission events.jsonl"),
            }
        }
        Ok(MissionEventsLoad {
            events,
            skipped_malformed_lines: skipped,
        })
    }

    pub fn replay_state(&self, id_or_latest: &str) -> Result<MissionState, anyhow::Error> {
        let events = self.load_events_lossy(id_or_latest)?.events;
        replay_state_from_events(&events)
            .ok_or_else(|| anyhow::anyhow!("mission has no replayable events"))
    }

    fn update_index(&self, mission: &Mission, state: &MissionState) -> Result<(), anyhow::Error> {
        std::fs::create_dir_all(&self.root)?;
        let mut summaries = self.list().unwrap_or_default();
        let summary = MissionSummary {
            id: mission.id.clone(),
            goal: mission.goal.clone(),
            dry_run: mission.dry_run,
            status: state.status.clone(),
            recommended_mode: mission.recommended_mode.clone(),
            created_at: mission.created_at,
            updated_at: mission.updated_at.max(state.updated_at).max(Utc::now()),
        };

        if let Some(existing) = summaries.iter_mut().find(|item| item.id == mission.id) {
            *existing = summary;
        } else {
            summaries.push(summary);
        }
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
        write_pretty_json(&self.index_path(), &summaries)?;
        Ok(())
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }
}

fn write_pretty_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), anyhow::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, anyhow::Error> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&content)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::MissionStatus;

    #[test]
    fn mission_store_creates_and_loads_dry_run() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = MissionStore::for_project(root.path());

        let bundle = store
            .create_dry_run(
                "refactor agent runtime and add tests".to_string(),
                root.path().into(),
            )
            .expect("create dry-run mission");

        assert!(store
            .mission_dir(&bundle.mission.id)
            .join("mission.json")
            .exists());
        assert_eq!(bundle.state.status, MissionStatus::Completed);
        assert_eq!(store.latest().expect("latest").id, bundle.mission.id);

        let loaded = store
            .load_bundle("latest", true)
            .expect("load latest bundle");
        assert_eq!(loaded.mission.id, bundle.mission.id);
        assert_eq!(loaded.events.len(), 3);
    }

    #[test]
    fn corrupt_final_jsonl_line_keeps_prior_events() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = MissionStore::for_project(root.path());
        let bundle = store
            .create_dry_run(
                "analyze src/agent architecture".to_string(),
                root.path().into(),
            )
            .expect("create dry-run mission");
        let path = store.mission_dir(&bundle.mission.id).join("events.jsonl");
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open events");
            use std::io::Write;
            writeln!(file, "{{not-json").expect("append corrupt line");
        }

        let loaded = store
            .load_events_lossy(&bundle.mission.id)
            .expect("load lossy events");
        assert_eq!(loaded.events.len(), 3);
        assert_eq!(loaded.skipped_malformed_lines, 1);
        assert_eq!(
            store
                .replay_state(&bundle.mission.id)
                .expect("replay state")
                .status,
            MissionStatus::Completed
        );
    }
}

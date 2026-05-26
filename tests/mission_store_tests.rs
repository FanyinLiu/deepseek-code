use deepseek_code::mission::MissionStatus;
use deepseek_code::storage::MissionStore;

#[test]
fn create_mission_dry_run_and_latest_resolves() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = MissionStore::for_project(root.path());

    let bundle = store
        .create_dry_run(
            "refactor src/agent safely and add tests".to_string(),
            root.path().to_path_buf(),
        )
        .expect("create mission");

    assert_eq!(bundle.state.status, MissionStatus::Completed);
    assert!(store
        .mission_dir(&bundle.mission.id)
        .join("plan.json")
        .exists());
    assert_eq!(
        store.resolve_id("latest").expect("resolve latest"),
        bundle.mission.id
    );
}

#[test]
fn status_and_replay_load_state() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = MissionStore::for_project(root.path());
    let bundle = store
        .create_dry_run(
            "analyze src/agent architecture".to_string(),
            root.path().to_path_buf(),
        )
        .expect("create mission");

    let state = store.load_state("latest").expect("load state");
    assert_eq!(state.mission_id, bundle.mission.id);
    assert_eq!(state.status, MissionStatus::Completed);

    let replayed = store.replay_state("latest").expect("replay state");
    assert_eq!(replayed.status, MissionStatus::Completed);
}

#[test]
fn corrupt_final_jsonl_line_does_not_destroy_prior_events() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = MissionStore::for_project(root.path());
    let bundle = store
        .create_dry_run(
            "review src/agent for safety".to_string(),
            root.path().to_path_buf(),
        )
        .expect("create mission");
    let events_path = store.mission_dir(&bundle.mission.id).join("events.jsonl");
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(events_path)
            .expect("open events");
        use std::io::Write;
        writeln!(file, "{{broken").expect("append broken final line");
    }

    let events = store
        .load_events_lossy("latest")
        .expect("load lossy events");

    assert_eq!(events.events.len(), 3);
    assert_eq!(events.skipped_malformed_lines, 1);
}

#[test]
fn lifecycle_events_replay_to_latest_state() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = MissionStore::for_project(root.path());
    let bundle = store
        .create_dry_run(
            "refactor src/mission lifecycle".to_string(),
            root.path().to_path_buf(),
        )
        .expect("create mission");

    store.start(&bundle.mission.id).expect("start mission");
    store.pause(&bundle.mission.id).expect("pause mission");
    store
        .add_note(&bundle.mission.id, "waiting for review".to_string())
        .expect("add note");

    let state = store
        .replay_state(&bundle.mission.id)
        .expect("replay state");
    assert_eq!(state.status, MissionStatus::Paused);

    let events = store
        .load_events_lossy(&bundle.mission.id)
        .expect("load events");
    assert!(events.events.iter().any(|event| matches!(
        event.kind,
        deepseek_code::mission::MissionEventKind::MissionNote { .. }
    )));
}

#[test]
fn save_bundle_without_events_preserves_replay_log() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = MissionStore::for_project(root.path());
    let bundle = store
        .create_dry_run(
            "preserve replay events while saving metadata".to_string(),
            root.path().to_path_buf(),
        )
        .expect("create mission");
    let partial = store
        .load_bundle(&bundle.mission.id, false)
        .expect("load without events");

    store
        .save_bundle(&partial)
        .expect("save partial bundle without events");

    let events = store
        .load_events_lossy(&bundle.mission.id)
        .expect("load events after partial save");
    assert_eq!(events.events.len(), 3);
    assert_eq!(
        store
            .replay_state(&bundle.mission.id)
            .expect("replay after partial save")
            .status,
        MissionStatus::Completed
    );
}

#[test]
fn concurrent_mission_creates_preserve_index_entries() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = std::sync::Arc::new(MissionStore::for_project(root.path()));
    let mut handles = Vec::new();

    for index in 0..16 {
        let store = std::sync::Arc::clone(&store);
        let project_root = root.path().to_path_buf();
        handles.push(std::thread::spawn(move || {
            store
                .create_dry_run(format!("mission-{index}"), project_root)
                .expect("create mission");
        }));
    }
    for handle in handles {
        handle.join().expect("join create thread");
    }

    let summaries = store.list().expect("list missions");
    assert_eq!(summaries.len(), 16);
}

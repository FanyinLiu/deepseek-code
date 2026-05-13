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

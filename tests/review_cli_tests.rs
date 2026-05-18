use std::process::Command;

fn octocode_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn review_rejects_zero_parallel_without_panicking() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("src")).expect("create src");
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}\n").expect("write source");

    let output = octocode_command(root.path())
        .args(["review", "--parallel", "0"])
        .output()
        .expect("run octocode review");

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("invalid value") || stderr.contains("must be at least 1"));
    assert!(!stderr.contains("panicked"));
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

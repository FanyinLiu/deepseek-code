use std::process::Command;

fn octocode_command(project_root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_octocode"));
    command.arg("-C").arg(project_root);
    command
}

#[test]
fn search_limit_zero_returns_no_results() {
    let root = tempfile::tempdir().expect("tempdir");
    let src = root.path().join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("needle_file.rs"), "fn main() {}\n").expect("write file");

    let output = octocode_command(root.path())
        .args(["search", "needle", "--limit", "0"])
        .output()
        .expect("run octocode search");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    assert!(stdout(&output).contains("No results found for: needle"));
}

#[test]
fn search_limit_one_can_return_code_match_when_no_filename_matches() {
    let root = tempfile::tempdir().expect("tempdir");
    let src = root.path().join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("main.rs"), "fn main() { println!(\"needle\"); }\n")
        .expect("write file");

    let output = octocode_command(root.path())
        .args(["search", "needle", "--limit", "1"])
        .output()
        .expect("run octocode search");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("Found 1 results for: needle"));
    assert!(
        stdout.replace('\\', "/").contains("src/main.rs:1:"),
        "stdout={stdout}"
    );
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// Tests for path security policies.
use deepseek_code::policy::paths;

#[test]
fn test_normal_file_not_blocked() {
    assert!(!paths::is_blocked_path(
        std::path::Path::new("/home/user/project/src/main.rs"),
        std::path::Path::new("/home/user/project"),
    ));
}

#[test]
fn test_env_file_is_blocked() {
    assert!(paths::is_blocked_path(
        std::path::Path::new("/home/user/project/.env"),
        std::path::Path::new("/home/user/project"),
    ));
}

#[test]
fn test_env_production_is_blocked() {
    assert!(paths::is_blocked_path(
        std::path::Path::new("/home/user/project/.env.production"),
        std::path::Path::new("/home/user/project"),
    ));
}

#[test]
fn test_ssh_directory_is_blocked() {
    let home = dirs::home_dir().expect("home dir not found");
    let ssh_key = home.join(".ssh").join("id_rsa");
    assert!(paths::is_blocked_path(
        &ssh_key,
        std::path::Path::new("/tmp/project"),
    ));
}

#[test]
fn test_aws_credentials_blocked() {
    let home = dirs::home_dir().expect("home dir not found");
    let creds = home.join(".aws").join("credentials");
    assert!(paths::is_blocked_path(
        &creds,
        std::path::Path::new("/tmp/project"),
    ));
}

#[test]
fn test_etc_is_blocked() {
    assert!(paths::is_blocked_path(
        std::path::Path::new("/etc/passwd"),
        std::path::Path::new("/home/project"),
    ));
}

#[test]
fn test_credentials_file_blocked() {
    assert!(paths::is_blocked_path(
        std::path::Path::new("/home/project/credentials.json"),
        std::path::Path::new("/home/project"),
    ));
}

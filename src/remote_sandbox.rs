use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use flate2::write::GzEncoder;
use flate2::Compression;
use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::{Component, Path};
use std::time::{Duration, Instant};
use tar::Builder;
use uuid::Uuid;
use walkdir::WalkDir;

const DEFAULT_GITHUB_API: &str = "https://api.github.com";
const DEFAULT_GITHUB_REF: &str = "main";
const DEFAULT_WORKFLOW: &str = "candidate-validation.yml";
const DEFAULT_WORKSPACE_WORKFLOW: &str = "candidate-validation-workspace.yml";
const DEFAULT_WORKSPACE_RELEASE_TAG: &str = "octocode-workspace-cache";
const DEFAULT_MAX_WORKFLOW_INPUT_BYTES: usize = 60_000;
const DEFAULT_MAX_WORKSPACE_ARCHIVE_BYTES: usize = 25 * 1024 * 1024;
const MAX_GITHUB_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_GITHUB_API_JSON_BYTES: usize = 1024 * 1024;
const MAX_VALIDATION_ARTIFACT_ZIP_BYTES: usize = 5 * 1024 * 1024;
const MAX_VALIDATION_RESULT_JSON_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxValidationResult {
    pub run_id: String,
    pub candidate_id: Option<String>,
    pub status: SandboxStatus,
    pub summary: String,
    pub command_results: Vec<SandboxCommandResult>,
    pub risk_flags: Vec<String>,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub artifact_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStatus {
    Passed,
    Failed,
    Blocked,
    Error,
}

impl SandboxStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxCommandResult {
    pub name: String,
    pub argv: Vec<String>,
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct ValidationCommandSpec {
    name: String,
    argv: Vec<String>,
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
enum SandboxBackend {
    Disabled,
    Http {
        url: String,
        token: Option<String>,
    },
    GitHubActions {
        repo: String,
        workflow: String,
        git_ref: String,
        token: String,
        api_base: String,
        max_input_bytes: usize,
        poll_seconds: u64,
        timeout_seconds: u64,
        source_repo: Option<String>,
        source_ref: String,
        git_workflow: String,
    },
    GitHubActionsWorkspace {
        repo: String,
        workflow: String,
        git_ref: String,
        token: String,
        api_base: String,
        poll_seconds: u64,
        timeout_seconds: u64,
        release_tag: String,
        max_archive_bytes: usize,
    },
}

#[derive(Debug, Clone)]
struct SandboxConfig {
    backend: SandboxBackend,
}

pub async fn validate_candidate(
    project_root: &Path,
    candidate_id: &str,
    patch: &str,
    full: bool,
) -> Result<SandboxValidationResult> {
    let config = SandboxConfig::from_env();
    let commands = validation_commands(project_root, full);
    let request_id = format!("sandbox-{}", Uuid::new_v4());
    let patch_base64 = general_purpose::STANDARD.encode(patch.as_bytes());
    match config.backend {
        SandboxBackend::Disabled => Ok(SandboxValidationResult::blocked(
            request_id,
            Some(candidate_id.to_string()),
            "remote sandbox is disabled; set OCTOCODE_SANDBOX_BACKEND".to_string(),
            Some("disabled".to_string()),
        )),
        SandboxBackend::Http { url, token } => {
            let workspace_bundle = create_workspace_bundle(project_root)?;
            validate_with_http(
                &url,
                token.as_deref(),
                &request_id,
                candidate_id,
                workspace_bundle,
                patch,
                commands,
            )
            .await
        }
        SandboxBackend::GitHubActions {
            repo,
            workflow,
            git_ref,
            token,
            api_base,
            max_input_bytes,
            poll_seconds,
            timeout_seconds,
            source_repo,
            source_ref,
            git_workflow,
        } => {
            let config = GitHubActionConfig {
                repo,
                workflow,
                git_ref,
                token,
                api_base,
                max_input_bytes,
                poll_seconds,
                timeout_seconds,
            };
            if let Some(source_repo) = source_repo {
                validate_with_github_actions_git(GitHubActionGitRequest {
                    config,
                    git_workflow,
                    source_repo,
                    source_ref,
                    request_id: request_id.clone(),
                    candidate_id: candidate_id.to_string(),
                    patch_base64,
                    commands,
                })
                .await
            } else {
                let workspace_bundle = create_workspace_bundle(project_root)?;
                validate_with_github_actions(
                    config,
                    &request_id,
                    candidate_id,
                    workspace_bundle,
                    patch_base64,
                    commands,
                )
                .await
            }
        }
        SandboxBackend::GitHubActionsWorkspace {
            repo,
            workflow,
            git_ref,
            token,
            api_base,
            poll_seconds,
            timeout_seconds,
            release_tag,
            max_archive_bytes,
        } => {
            let workspace_archive = create_workspace_archive(project_root)?;
            if workspace_archive.bytes.len() > max_archive_bytes {
                return Ok(SandboxValidationResult::blocked(
                    request_id,
                    Some(candidate_id.to_string()),
                    format!(
                        "workspace archive is too large: {} > {max_archive_bytes} bytes",
                        workspace_archive.bytes.len()
                    ),
                    Some("github_actions_workspace".to_string()),
                ));
            }
            validate_with_github_actions_workspace(GitHubActionWorkspaceRequest {
                config: GitHubActionConfig {
                    repo,
                    workflow,
                    git_ref,
                    token,
                    api_base,
                    max_input_bytes: DEFAULT_MAX_WORKFLOW_INPUT_BYTES,
                    poll_seconds,
                    timeout_seconds,
                },
                release_tag,
                request_id,
                candidate_id: candidate_id.to_string(),
                workspace_archive,
                patch_base64,
                commands,
            })
            .await
        }
    }
}

impl SandboxValidationResult {
    pub fn blocked(
        run_id: String,
        candidate_id: Option<String>,
        summary: String,
        backend: Option<String>,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            run_id,
            candidate_id,
            status: SandboxStatus::Blocked,
            summary,
            command_results: Vec::new(),
            risk_flags: vec!["sandbox_blocked".to_string()],
            started_at_unix_ms: now,
            finished_at_unix_ms: now,
            backend,
            artifact_url: None,
        }
    }
}

impl SandboxConfig {
    fn from_env() -> Self {
        match std::env::var("OCTOCODE_SANDBOX_BACKEND")
            .unwrap_or_else(|_| "disabled".to_string())
            .as_str()
        {
            "github_actions" => {
                let token = std::env::var("OCTOCODE_SANDBOX_TOKEN")
                    .or_else(|_| std::env::var("GITHUB_TOKEN"));
                match (std::env::var("OCTOCODE_SANDBOX_REPO"), token) {
                    (Ok(repo), Ok(token))
                        if !repo.trim().is_empty() && !token.trim().is_empty() =>
                    {
                        Self {
                            backend: SandboxBackend::GitHubActions {
                                repo,
                                workflow: std::env::var("OCTOCODE_SANDBOX_WORKFLOW")
                                    .unwrap_or_else(|_| DEFAULT_WORKFLOW.to_string()),
                                git_ref: std::env::var("OCTOCODE_SANDBOX_REF")
                                    .unwrap_or_else(|_| DEFAULT_GITHUB_REF.to_string()),
                                token,
                                api_base: std::env::var("OCTOCODE_GITHUB_API")
                                    .unwrap_or_else(|_| DEFAULT_GITHUB_API.to_string()),
                                max_input_bytes: parse_env_usize(
                                    "OCTOCODE_SANDBOX_WORKFLOW_INPUT_MAX_BYTES",
                                    DEFAULT_MAX_WORKFLOW_INPUT_BYTES,
                                ),
                                poll_seconds: parse_env_u64("OCTOCODE_SANDBOX_POLL_SECONDS", 5),
                                timeout_seconds: parse_env_u64(
                                    "OCTOCODE_SANDBOX_TIMEOUT_SECONDS",
                                    900,
                                ),
                                source_repo: std::env::var("OCTOCODE_SANDBOX_SOURCE_REPO")
                                    .ok()
                                    .filter(|value| !value.trim().is_empty()),
                                source_ref: std::env::var("OCTOCODE_SANDBOX_SOURCE_REF")
                                    .unwrap_or_else(|_| DEFAULT_GITHUB_REF.to_string()),
                                git_workflow: std::env::var("OCTOCODE_SANDBOX_GIT_WORKFLOW")
                                    .unwrap_or_else(|_| "candidate-validation-git.yml".to_string()),
                            },
                        }
                    }
                    _ => Self {
                        backend: SandboxBackend::Disabled,
                    },
                }
            }
            "github_actions_workspace" => {
                let token = std::env::var("OCTOCODE_SANDBOX_TOKEN")
                    .or_else(|_| std::env::var("GITHUB_TOKEN"));
                match (std::env::var("OCTOCODE_SANDBOX_REPO"), token) {
                    (Ok(repo), Ok(token))
                        if !repo.trim().is_empty() && !token.trim().is_empty() =>
                    {
                        Self {
                            backend: SandboxBackend::GitHubActionsWorkspace {
                                repo,
                                workflow: std::env::var("OCTOCODE_SANDBOX_WORKSPACE_WORKFLOW")
                                    .unwrap_or_else(|_| DEFAULT_WORKSPACE_WORKFLOW.to_string()),
                                git_ref: std::env::var("OCTOCODE_SANDBOX_REF")
                                    .unwrap_or_else(|_| DEFAULT_GITHUB_REF.to_string()),
                                token,
                                api_base: std::env::var("OCTOCODE_GITHUB_API")
                                    .unwrap_or_else(|_| DEFAULT_GITHUB_API.to_string()),
                                poll_seconds: parse_env_u64("OCTOCODE_SANDBOX_POLL_SECONDS", 5),
                                timeout_seconds: parse_env_u64(
                                    "OCTOCODE_SANDBOX_TIMEOUT_SECONDS",
                                    900,
                                ),
                                release_tag: std::env::var(
                                    "OCTOCODE_SANDBOX_WORKSPACE_RELEASE_TAG",
                                )
                                .unwrap_or_else(|_| DEFAULT_WORKSPACE_RELEASE_TAG.to_string()),
                                max_archive_bytes: parse_env_usize(
                                    "OCTOCODE_SANDBOX_WORKSPACE_MAX_BYTES",
                                    DEFAULT_MAX_WORKSPACE_ARCHIVE_BYTES,
                                ),
                            },
                        }
                    }
                    _ => Self {
                        backend: SandboxBackend::Disabled,
                    },
                }
            }
            "http" => match std::env::var("OCTOCODE_SANDBOX_URL") {
                Ok(url) if !url.trim().is_empty() => Self {
                    backend: SandboxBackend::Http {
                        url,
                        token: std::env::var("OCTOCODE_SANDBOX_TOKEN")
                            .ok()
                            .filter(|token| !token.is_empty()),
                    },
                },
                _ => Self {
                    backend: SandboxBackend::Disabled,
                },
            },
            _ => Self {
                backend: SandboxBackend::Disabled,
            },
        }
    }
}

async fn validate_with_http(
    url: &str,
    token: Option<&str>,
    request_id: &str,
    candidate_id: &str,
    workspace_bundle: String,
    patch: &str,
    commands: Vec<ValidationCommandSpec>,
) -> Result<SandboxValidationResult> {
    let client = reqwest::Client::new();
    let mut request = client
        .post(format!("{}/v1/validations", url.trim_end_matches('/')))
        .header(USER_AGENT, "octocode-remote-sandbox")
        .json(&serde_json::json!({
            "request_id": request_id,
            "candidate_id": candidate_id,
            "source": {
                "kind": "archive_base64",
                "format": "tar_gz",
                "content": workspace_bundle,
            },
            "patch": {
                "format": "unified_diff",
                "content": patch,
            },
            "validation": {
                "commands": commands,
            },
            "policy": {
                "network": "disabled",
                "max_output_bytes": 120000,
            },
        }));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .context("send sandbox validation request")?
        .error_for_status()
        .context("sandbox validation request failed")?;
    let mut result: SandboxValidationResult =
        response_json_capped(response, MAX_VALIDATION_RESULT_JSON_BYTES)
            .await
            .context("parse sandbox validation response")?;
    result.backend = Some("http".to_string());
    Ok(result)
}

struct GitHubActionGitRequest {
    config: GitHubActionConfig,
    git_workflow: String,
    source_repo: String,
    source_ref: String,
    request_id: String,
    candidate_id: String,
    patch_base64: String,
    commands: Vec<ValidationCommandSpec>,
}

struct GitHubActionWorkspaceRequest {
    config: GitHubActionConfig,
    release_tag: String,
    request_id: String,
    candidate_id: String,
    workspace_archive: WorkspaceArchive,
    patch_base64: String,
    commands: Vec<ValidationCommandSpec>,
}

struct GitHubActionWorkspaceDispatch<'a> {
    client: &'a reqwest::Client,
    config: &'a GitHubActionConfig,
    api_base: &'a str,
    request_id: &'a str,
    candidate_id: &'a str,
    workspace_asset_url: &'a str,
    workspace_archive_sha256: &'a str,
    patch_base64: &'a str,
    commands_json: &'a str,
}

async fn validate_with_github_actions_git(
    request: GitHubActionGitRequest,
) -> Result<SandboxValidationResult> {
    let GitHubActionGitRequest {
        mut config,
        git_workflow,
        source_repo,
        source_ref,
        request_id,
        candidate_id,
        patch_base64,
        commands,
    } = request;
    let commands_json = serde_json::to_string(&commands)?;
    let input_bytes =
        patch_base64.len() + commands_json.len() + source_repo.len() + source_ref.len();
    if input_bytes > config.max_input_bytes {
        return Ok(SandboxValidationResult::blocked(
            request_id.clone(),
            Some(candidate_id.clone()),
            format!(
                "GitHub Actions git workflow payload is too large: {input_bytes} > {} bytes",
                config.max_input_bytes
            ),
            Some("github_actions_git".to_string()),
        ));
    }

    config.workflow = git_workflow;
    let client = github_client(&config.token)?;
    let api_base = config.api_base.trim_end_matches('/');
    let dispatch_url = format!(
        "{api_base}/repos/{}/actions/workflows/{}/dispatches",
        config.repo, config.workflow
    );
    let dispatched_at = chrono::Utc::now();
    let response = client
        .post(dispatch_url)
        .json(&serde_json::json!({
            "ref": config.git_ref,
            "inputs": {
                "request_id": request_id,
                "candidate_id": candidate_id,
                "source_repo": source_repo,
                "source_ref": source_ref,
                "patch_base64": patch_base64,
                "validation_commands_json": commands_json,
            }
        }))
        .send()
        .await
        .context("dispatch GitHub Actions git sandbox workflow")?;
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("GitHub Actions git dispatch failed: {status} {body}");
    }

    let run = wait_for_workflow_run(&client, &config, api_base, &request_id, dispatched_at).await?;
    let mut result =
        download_validation_artifact(&client, api_base, &config.repo, run.id, &request_id).await?;
    result.backend = Some("github_actions_git".to_string());
    result.artifact_url = run.html_url;
    Ok(result)
}

async fn validate_with_github_actions_workspace(
    request: GitHubActionWorkspaceRequest,
) -> Result<SandboxValidationResult> {
    let GitHubActionWorkspaceRequest {
        config,
        release_tag,
        request_id,
        candidate_id,
        workspace_archive,
        patch_base64,
        commands,
    } = request;
    let commands_json = serde_json::to_string(&commands)?;
    let client = github_client(&config.token)?;
    let api_base = config.api_base.trim_end_matches('/');
    let asset_name = format!("{request_id}-{candidate_id}.tar.gz");
    let uploaded_asset = upload_workspace_asset(
        &client,
        api_base,
        &config.repo,
        &release_tag,
        &asset_name,
        &workspace_archive.bytes,
    )
    .await?;

    let validation = dispatch_workspace_validation(GitHubActionWorkspaceDispatch {
        client: &client,
        config: &config,
        api_base,
        request_id: &request_id,
        candidate_id: &candidate_id,
        workspace_asset_url: &uploaded_asset.url,
        workspace_archive_sha256: &workspace_archive.sha256,
        patch_base64: &patch_base64,
        commands_json: &commands_json,
    })
    .await;
    let cleanup = delete_release_asset(&client, api_base, &config.repo, uploaded_asset.id).await;
    let mut result = validation?;
    if let Err(err) = cleanup {
        result
            .risk_flags
            .push(format!("workspace_asset_cleanup_failed: {err}"));
    }
    result.backend = Some("github_actions_workspace".to_string());
    Ok(result)
}

async fn dispatch_workspace_validation(
    request: GitHubActionWorkspaceDispatch<'_>,
) -> Result<SandboxValidationResult> {
    let GitHubActionWorkspaceDispatch {
        client,
        config,
        api_base,
        request_id,
        candidate_id,
        workspace_asset_url,
        workspace_archive_sha256,
        patch_base64,
        commands_json,
    } = request;
    let dispatch_url = format!(
        "{api_base}/repos/{}/actions/workflows/{}/dispatches",
        config.repo, config.workflow
    );
    let dispatched_at = chrono::Utc::now();
    let response = client
        .post(dispatch_url)
        .json(&serde_json::json!({
            "ref": config.git_ref,
            "inputs": {
                "request_id": request_id,
                "candidate_id": candidate_id,
                "workspace_asset_url": workspace_asset_url,
                "workspace_archive_sha256": workspace_archive_sha256,
                "patch_base64": patch_base64,
                "validation_commands_json": commands_json,
            }
        }))
        .send()
        .await
        .context("dispatch GitHub Actions workspace sandbox workflow")?;
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("GitHub Actions workspace dispatch failed: {status} {body}");
    }

    let run = wait_for_workflow_run(client, config, api_base, request_id, dispatched_at).await?;
    let mut result =
        download_validation_artifact(client, api_base, &config.repo, run.id, request_id).await?;
    result.artifact_url = run.html_url;
    Ok(result)
}

#[derive(Debug, Deserialize)]
struct Release {
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    id: u64,
    url: String,
}

async fn upload_workspace_asset(
    client: &reqwest::Client,
    api_base: &str,
    repo: &str,
    release_tag: &str,
    asset_name: &str,
    archive_bytes: &[u8],
) -> Result<ReleaseAsset> {
    let release = ensure_release(client, api_base, repo, release_tag).await?;
    let upload_base = release
        .upload_url
        .split('{')
        .next()
        .unwrap_or(release.upload_url.as_str());
    let upload_url = format!("{upload_base}?name={asset_name}");
    let response = client
        .post(upload_url)
        .header(CONTENT_TYPE, "application/gzip")
        .body(archive_bytes.to_vec())
        .send()
        .await
        .context("upload workspace archive asset")?;
    if response.status() != reqwest::StatusCode::CREATED {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("workspace asset upload failed: {status} {body}");
    }
    response_json_capped(response, MAX_GITHUB_API_JSON_BYTES)
        .await
        .context("parse uploaded workspace asset")
}

async fn ensure_release(
    client: &reqwest::Client,
    api_base: &str,
    repo: &str,
    release_tag: &str,
) -> Result<Release> {
    let tag_url = format!("{api_base}/repos/{repo}/releases/tags/{release_tag}");
    let response = client.get(&tag_url).send().await?;
    if response.status().is_success() {
        return response_json_capped(response, MAX_GITHUB_API_JSON_BYTES)
            .await
            .context("parse workspace cache release");
    }
    if response.status() != reqwest::StatusCode::NOT_FOUND {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("workspace cache release lookup failed: {status} {body}");
    }
    let create_url = format!("{api_base}/repos/{repo}/releases");
    let response = client
        .post(create_url)
        .json(&serde_json::json!({
            "tag_name": release_tag,
            "name": "Octocode Workspace Cache",
            "body": "Temporary workspace archives for Octocode sandbox validation.",
            "draft": false,
            "prerelease": true,
        }))
        .send()
        .await
        .context("create workspace cache release")?;
    if response.status() != reqwest::StatusCode::CREATED {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("workspace cache release creation failed: {status} {body}");
    }
    response_json_capped(response, MAX_GITHUB_API_JSON_BYTES)
        .await
        .context("parse created workspace cache release")
}

async fn delete_release_asset(
    client: &reqwest::Client,
    api_base: &str,
    repo: &str,
    asset_id: u64,
) -> Result<()> {
    let url = format!("{api_base}/repos/{repo}/releases/assets/{asset_id}");
    let response = client.delete(url).send().await?;
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("workspace asset cleanup failed: {status} {body}");
    }
    Ok(())
}

struct GitHubActionConfig {
    repo: String,
    workflow: String,
    git_ref: String,
    token: String,
    api_base: String,
    max_input_bytes: usize,
    poll_seconds: u64,
    timeout_seconds: u64,
}

async fn validate_with_github_actions(
    config: GitHubActionConfig,
    request_id: &str,
    candidate_id: &str,
    workspace_bundle: String,
    patch_base64: String,
    commands: Vec<ValidationCommandSpec>,
) -> Result<SandboxValidationResult> {
    let commands_json = serde_json::to_string(&commands)?;
    let input_bytes = workspace_bundle.len() + patch_base64.len() + commands_json.len();
    if input_bytes > config.max_input_bytes {
        return Ok(SandboxValidationResult::blocked(
            request_id.to_string(),
            Some(candidate_id.to_string()),
            format!(
                "GitHub Actions workflow_dispatch payload is too large: {input_bytes} > {} bytes; use HTTP runner or object storage",
                config.max_input_bytes
            ),
            Some("github_actions".to_string()),
        ));
    }

    let client = github_client(&config.token)?;
    let api_base = config.api_base.trim_end_matches('/');
    let dispatch_url = format!(
        "{api_base}/repos/{}/actions/workflows/{}/dispatches",
        config.repo, config.workflow
    );
    let dispatched_at = chrono::Utc::now();
    let response = client
        .post(dispatch_url)
        .json(&serde_json::json!({
            "ref": config.git_ref,
            "inputs": {
                "request_id": request_id,
                "candidate_id": candidate_id,
                "workspace_bundle_base64": workspace_bundle,
                "patch_base64": patch_base64,
                "validation_commands_json": commands_json,
            }
        }))
        .send()
        .await
        .context("dispatch GitHub Actions sandbox workflow")?;
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        let status = response.status();
        let body = response_text_capped(response, MAX_GITHUB_ERROR_BODY_BYTES).await;
        bail!("GitHub Actions dispatch failed: {status} {body}");
    }

    let run = wait_for_workflow_run(&client, &config, api_base, request_id, dispatched_at).await?;
    let mut result =
        download_validation_artifact(&client, api_base, &config.repo, run.id, request_id).await?;
    result.backend = Some("github_actions".to_string());
    result.artifact_url = run.html_url;
    Ok(result)
}

#[derive(Debug, Deserialize)]
struct WorkflowRunsResponse {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    id: u64,
    status: String,
    conclusion: Option<String>,
    display_title: Option<String>,
    created_at: DateTimeCompat,
    html_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct DateTimeCompat(chrono::DateTime<chrono::Utc>);

async fn wait_for_workflow_run(
    client: &reqwest::Client,
    config: &GitHubActionConfig,
    api_base: &str,
    request_id: &str,
    dispatched_at: chrono::DateTime<chrono::Utc>,
) -> Result<WorkflowRun> {
    let started = Instant::now();
    loop {
        let runs_url = format!(
            "{api_base}/repos/{}/actions/workflows/{}/runs?event=workflow_dispatch&per_page=20",
            config.repo, config.workflow
        );
        let response = client.get(runs_url).send().await?.error_for_status()?;
        let response: WorkflowRunsResponse =
            response_json_capped(response, MAX_GITHUB_API_JSON_BYTES).await?;
        if let Some(run) = response.workflow_runs.into_iter().find(|run| {
            run.created_at.0 >= dispatched_at - chrono::Duration::seconds(5)
                && run
                    .display_title
                    .as_deref()
                    .is_some_and(|title| title.contains(request_id))
        }) {
            if run.status == "completed" {
                if run.conclusion.as_deref() == Some("success") {
                    return Ok(run);
                }
                return Err(anyhow!(
                    "GitHub Actions sandbox workflow completed with conclusion {:?}",
                    run.conclusion
                ));
            }
        }
        if started.elapsed() > Duration::from_secs(config.timeout_seconds) {
            bail!("timed out waiting for GitHub Actions sandbox workflow");
        }
        tokio::time::sleep(Duration::from_secs(config.poll_seconds)).await;
    }
}

#[derive(Debug, Deserialize)]
struct ArtifactsResponse {
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    name: String,
    archive_download_url: String,
}

async fn download_validation_artifact(
    client: &reqwest::Client,
    api_base: &str,
    repo: &str,
    run_id: u64,
    request_id: &str,
) -> Result<SandboxValidationResult> {
    let artifacts_url = format!("{api_base}/repos/{repo}/actions/runs/{run_id}/artifacts");
    let response = client.get(artifacts_url).send().await?.error_for_status()?;
    let response: ArtifactsResponse =
        response_json_capped(response, MAX_GITHUB_API_JSON_BYTES).await?;
    let expected = format!("validation-result-{request_id}");
    let artifact = response
        .artifacts
        .into_iter()
        .find(|artifact| artifact.name == expected)
        .ok_or_else(|| anyhow!("validation artifact not found: {expected}"))?;
    let response = client
        .get(artifact.archive_download_url)
        .send()
        .await?
        .error_for_status()?;
    let bytes = response_bytes_capped(response, MAX_VALIDATION_ARTIFACT_ZIP_BYTES).await?;
    parse_validation_artifact_zip(bytes)
}

fn parse_validation_artifact_zip(bytes: Vec<u8>) -> Result<SandboxValidationResult> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("open validation artifact zip")?;
    let file = archive
        .by_name("validation-result.json")
        .context("read validation-result.json from artifact")?;
    if file.size() > MAX_VALIDATION_RESULT_JSON_BYTES as u64 {
        bail!(
            "validation-result.json too large: {} bytes exceeds {MAX_VALIDATION_RESULT_JSON_BYTES} byte limit",
            file.size()
        );
    }
    let mut data = String::new();
    let bytes_read = file
        .take((MAX_VALIDATION_RESULT_JSON_BYTES + 1) as u64)
        .read_to_string(&mut data)?;
    if bytes_read > MAX_VALIDATION_RESULT_JSON_BYTES {
        bail!(
            "validation-result.json too large: {bytes_read} bytes exceeds {MAX_VALIDATION_RESULT_JSON_BYTES} byte limit"
        );
    }
    serde_json::from_str(&data).context("parse validation-result.json")
}

async fn response_text_capped(response: reqwest::Response, max_bytes: usize) -> String {
    match response_bytes_capped(response, max_bytes).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(error) => format!("[response body unavailable: {error}]"),
    }
}

async fn response_json_capped<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<T> {
    let bytes = response_bytes_capped(response, max_bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn response_bytes_capped(response: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>> {
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes as u64 {
            bail!("response too large: {content_length} bytes exceeds {max_bytes} byte limit");
        }
    }

    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            bail!("response too large: exceeds {max_bytes} byte limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn github_client(token: &str) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(USER_AGENT, "octocode-remote-sandbox".parse()?);
    headers.insert(ACCEPT, "application/vnd.github+json".parse()?);
    headers.insert(CONTENT_TYPE, "application/json".parse()?);
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .context("invalid GitHub authorization token")?,
    );
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

fn validation_commands(project_root: &Path, full: bool) -> Vec<ValidationCommandSpec> {
    if !project_root.join("Cargo.toml").exists() {
        return Vec::new();
    }
    let mut commands = vec![ValidationCommandSpec {
        name: "cargo_check".to_string(),
        argv: vec![
            "cargo".to_string(),
            "check".to_string(),
            "--all-targets".to_string(),
            "--all-features".to_string(),
        ],
        timeout_seconds: Some(300),
    }];
    if full {
        commands.push(ValidationCommandSpec {
            name: "cargo_test".to_string(),
            argv: vec![
                "cargo".to_string(),
                "test".to_string(),
                "--all-targets".to_string(),
                "--all-features".to_string(),
            ],
            timeout_seconds: Some(900),
        });
    }
    commands
}

struct WorkspaceArchive {
    bytes: Vec<u8>,
    sha256: String,
}

fn create_workspace_bundle(project_root: &Path) -> Result<String> {
    Ok(general_purpose::STANDARD.encode(create_workspace_archive(project_root)?.bytes))
}

fn create_workspace_archive(project_root: &Path) -> Result<WorkspaceArchive> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);
    for entry in WalkDir::new(project_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded(project_root, entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(project_root)?;
        validate_archive_path(relative)?;
        builder.append_path_with_name(entry.path(), relative)?;
    }
    let encoder = builder.into_inner()?;
    let bytes = encoder.finish()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(WorkspaceArchive {
        sha256: format!("{:x}", hasher.finalize()),
        bytes,
    })
}

fn is_excluded(project_root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return true;
    };
    let Some(first) = relative.components().next() else {
        return false;
    };
    let Component::Normal(name) = first else {
        return true;
    };
    matches!(
        name.to_str(),
        Some(
            ".git"
                | "target"
                | ".octocode"
                | ".deepseek-code"
                | "node_modules"
                | "logs"
                | ".DS_Store"
        )
    ) || name
        .to_str()
        .is_some_and(|name| name == ".env" || name.starts_with(".env."))
}

fn validate_archive_path(path: &Path) -> Result<()> {
    if path.is_absolute() {
        bail!("absolute workspace path is not allowed");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("unsafe workspace path: {}", path.display())
            }
        }
    }
    Ok(())
}

fn parse_env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn zip_with_validation_json(content: &[u8]) -> Vec<u8> {
        use std::io::Write as _;

        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(
                "validation-result.json",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("start zip file");
        writer.write_all(content).expect("write zip content");
        writer.finish().expect("finish zip").into_inner()
    }

    #[test]
    fn parse_validation_artifact_zip_reads_result() {
        let json = serde_json::json!({
            "run_id": "run-1",
            "candidate_id": null,
            "status": "passed",
            "summary": "ok",
            "command_results": [],
            "risk_flags": [],
            "started_at_unix_ms": 1,
            "finished_at_unix_ms": 2
        });

        let result =
            parse_validation_artifact_zip(zip_with_validation_json(json.to_string().as_bytes()))
                .expect("parse validation artifact");

        assert_eq!(result.run_id, "run-1");
        assert_eq!(result.status, SandboxStatus::Passed);
    }

    #[test]
    fn parse_validation_artifact_zip_rejects_large_result_json() {
        let oversized = vec![b' '; MAX_VALIDATION_RESULT_JSON_BYTES + 1];
        let error = parse_validation_artifact_zip(zip_with_validation_json(&oversized))
            .expect_err("large validation result should fail");

        assert!(
            error
                .to_string()
                .contains("validation-result.json too large"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn response_json_capped_rejects_large_remote_sandbox_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(9)))
            .mount(&server)
            .await;

        let response = reqwest::Client::new()
            .get(server.uri())
            .send()
            .await
            .expect("mock response");
        let error = response_json_capped::<serde_json::Value>(response, 8)
            .await
            .expect_err("body over limit should fail");

        assert!(
            error.to_string().contains("response too large"),
            "{error:?}"
        );
    }
}

//! Local process sandbox for applying mutation artifacts into a temporary workspace copy.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use async_trait::async_trait;
use oris_evolution::{ArtifactEncoding, BlastRadius, MutationId, MutationTarget, PreparedMutation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::time::{timeout, Duration};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub allowed_programs: Vec<String>,
    pub max_duration_ms: u64,
    pub max_output_bytes: usize,
    pub denied_env_prefixes: Vec<String>,
}

impl SandboxPolicy {
    pub fn oris_default() -> Self {
        Self {
            allowed_programs: vec!["cargo".into(), "rustc".into(), "git".into()],
            max_duration_ms: 300_000,
            max_output_bytes: 1_048_576,
            denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
        }
    }
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self::oris_default()
    }
}

#[derive(Clone, Debug)]
pub struct SandboxReceipt {
    pub mutation_id: MutationId,
    pub workdir: PathBuf,
    pub applied: bool,
    pub changed_files: Vec<PathBuf>,
    pub patch_hash: String,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CommandExecution {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("unsupported artifact encoding")]
    UnsupportedArtifact,
    #[error("mutation target violation: {0}")]
    TargetViolation(String),
    #[error("sandbox I/O error: {0}")]
    Io(String),
    #[error("command denied by policy: {0}")]
    CommandDenied(String),
    #[error("command timed out: {0}")]
    Timeout(String),
    #[error("patch rejected: {0}")]
    PatchRejected(String),
    #[error("patch apply failed: {0}")]
    PatchApplyFailed(String),
}

#[async_trait]
pub trait Sandbox: Send + Sync {
    async fn apply(
        &self,
        mutation: &PreparedMutation,
        policy: &SandboxPolicy,
    ) -> Result<SandboxReceipt, SandboxError>;
}

pub struct LocalProcessSandbox {
    run_id: String,
    workspace_root: PathBuf,
    temp_root: PathBuf,
}

impl LocalProcessSandbox {
    pub fn new<P: Into<PathBuf>, Q: Into<PathBuf>, S: Into<String>>(
        run_id: S,
        workspace_root: P,
        temp_root: Q,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            workspace_root: workspace_root.into(),
            temp_root: temp_root.into(),
        }
    }

    fn sandbox_base(&self, mutation_id: &str) -> PathBuf {
        self.temp_root.join(&self.run_id).join(mutation_id)
    }
}

#[async_trait]
impl Sandbox for LocalProcessSandbox {
    async fn apply(
        &self,
        mutation: &PreparedMutation,
        policy: &SandboxPolicy,
    ) -> Result<SandboxReceipt, SandboxError> {
        if !matches!(mutation.artifact.encoding, ArtifactEncoding::UnifiedDiff) {
            return Err(SandboxError::UnsupportedArtifact);
        }

        let changed_files = parse_changed_files(&mutation.artifact.payload);
        validate_target(&mutation.intent.target, &changed_files)?;

        let base_dir = self.sandbox_base(&mutation.intent.id);
        let repo_dir = base_dir.join("repo");
        let patch_path = base_dir.join("patch.diff");
        let stdout_log = base_dir.join("apply.stdout.log");
        let stderr_log = base_dir.join("apply.stderr.log");

        if base_dir.exists() {
            fs::remove_dir_all(&base_dir).map_err(io_err)?;
        }
        fs::create_dir_all(&repo_dir).map_err(io_err)?;
        copy_workspace(&self.workspace_root, &repo_dir)?;
        fs::write(&patch_path, mutation.artifact.payload.as_bytes()).map_err(io_err)?;

        let check = execute_allowed_command(
            policy,
            &repo_dir,
            "git",
            &[
                String::from("apply"),
                String::from("--check"),
                patch_path.to_string_lossy().to_string(),
            ],
            policy.max_duration_ms,
        )
        .await?;
        if !check.success {
            fs::write(&stdout_log, check.stdout.as_bytes()).map_err(io_err)?;
            fs::write(&stderr_log, check.stderr.as_bytes()).map_err(io_err)?;
            return Err(SandboxError::PatchRejected(if check.stderr.is_empty() {
                check.stdout
            } else {
                check.stderr
            }));
        }

        let apply = execute_allowed_command(
            policy,
            &repo_dir,
            "git",
            &[
                String::from("apply"),
                patch_path.to_string_lossy().to_string(),
            ],
            policy.max_duration_ms,
        )
        .await?;

        fs::write(&stdout_log, apply.stdout.as_bytes()).map_err(io_err)?;
        fs::write(&stderr_log, apply.stderr.as_bytes()).map_err(io_err)?;
        if !apply.success {
            return Err(SandboxError::PatchApplyFailed(if apply.stderr.is_empty() {
                apply.stdout
            } else {
                apply.stderr
            }));
        }

        Ok(SandboxReceipt {
            mutation_id: mutation.intent.id.clone(),
            workdir: repo_dir,
            applied: true,
            changed_files,
            patch_hash: hash_patch(&mutation.artifact.payload),
            stdout_log,
            stderr_log,
        })
    }
}

pub async fn execute_allowed_command(
    policy: &SandboxPolicy,
    workdir: &Path,
    program: &str,
    args: &[String],
    timeout_ms: u64,
) -> Result<CommandExecution, SandboxError> {
    if !policy
        .allowed_programs
        .iter()
        .any(|allowed| allowed == program)
    {
        return Err(SandboxError::CommandDenied(program.to_string()));
    }

    let started = Instant::now();
    let mut command = tokio::process::Command::new(program);
    command.kill_on_drop(true);
    command.args(args);
    command.current_dir(workdir);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    for (key, _) in std::env::vars() {
        if policy
            .denied_env_prefixes
            .iter()
            .any(|prefix| key.to_ascii_uppercase().contains(prefix))
        {
            command.env_remove(&key);
        }
    }

    let output = timeout(Duration::from_millis(timeout_ms), command.output())
        .await
        .map_err(|_| SandboxError::Timeout(format!("{program} {}", args.join(" "))))?
        .map_err(io_err)?;

    Ok(CommandExecution {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: truncate_to_limit(&output.stdout, policy.max_output_bytes),
        stderr: truncate_to_limit(&output.stderr, policy.max_output_bytes),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

pub fn parse_changed_files(patch: &str) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let mut parts = rest.split_whitespace();
            let _lhs = parts.next();
            if let Some(rhs) = parts.next() {
                let rhs = rhs.trim_start_matches("b/");
                if rhs != "/dev/null" {
                    files.insert(PathBuf::from(rhs));
                }
            }
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            let path = rest.trim();
            if path != "/dev/null" {
                files.insert(PathBuf::from(path.trim_start_matches("b/")));
            }
        }
    }
    files.into_iter().collect()
}

pub fn count_changed_lines(patch: &str) -> usize {
    patch
        .lines()
        .filter(|line| {
            (line.starts_with('+') || line.starts_with('-'))
                && !line.starts_with("+++")
                && !line.starts_with("---")
        })
        .count()
}

pub fn compute_blast_radius(patch: &str) -> BlastRadius {
    BlastRadius {
        files_changed: parse_changed_files(patch).len(),
        lines_changed: count_changed_lines(patch),
    }
}

pub fn validate_target(
    target: &MutationTarget,
    changed_files: &[PathBuf],
) -> Result<(), SandboxError> {
    let allowed_prefixes = match target {
        MutationTarget::WorkspaceRoot => Vec::new(),
        MutationTarget::Crate { name } => vec![format!("crates/{name}")],
        MutationTarget::Paths { allow } => allow.clone(),
    };

    if allowed_prefixes.is_empty() {
        return Ok(());
    }

    for file in changed_files {
        let path = file.to_string_lossy();
        let allowed = allowed_prefixes
            .iter()
            .any(|prefix| path.as_ref() == prefix || path.starts_with(&format!("{prefix}/")));
        if !allowed {
            return Err(SandboxError::TargetViolation(path.into_owned()));
        }
    }
    Ok(())
}

fn copy_workspace(src: &Path, dst: &Path) -> Result<(), SandboxError> {
    for entry in fs::read_dir(src).map_err(io_err)? {
        let entry = entry.map_err(io_err)?;
        let source_path = entry.path();
        let target_path = dst.join(entry.file_name());
        let file_type = entry.file_type().map_err(io_err)?;
        if should_skip(&source_path) {
            continue;
        }
        if file_type.is_dir() {
            fs::create_dir_all(&target_path).map_err(io_err)?;
            copy_workspace(&source_path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path).map_err(io_err)?;
        }
    }
    Ok(())
}

fn should_skip(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        // Skip generated artifact roots to keep sandbox copies bounded.
        .map(|name| matches!(name, ".git" | "target" | "demo_runs"))
        .unwrap_or(false)
}

fn truncate_to_limit(bytes: &[u8], max_output_bytes: usize) -> String {
    let limit = bytes.len().min(max_output_bytes);
    String::from_utf8_lossy(&bytes[..limit]).into_owned()
}

fn hash_patch(payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

fn io_err(err: std::io::Error) -> SandboxError {
    SandboxError::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_evolution::{MutationArtifact, MutationIntent, RiskLevel};

    fn temp_workspace(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("oris-sandbox-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("src/lib.rs"), "pub fn hello() {}\n").unwrap();
        path
    }

    #[tokio::test]
    async fn command_runner_rejects_non_allowlisted_program() {
        let workspace = temp_workspace("policy");
        let policy = SandboxPolicy {
            allowed_programs: vec!["git".into()],
            max_duration_ms: 1_000,
            max_output_bytes: 1024,
            denied_env_prefixes: Vec::new(),
        };
        let result = execute_allowed_command(&policy, &workspace, "cargo", &[], 1_000).await;
        assert!(matches!(result, Err(SandboxError::CommandDenied(_))));
    }

    #[tokio::test]
    async fn sandbox_rejects_patch_outside_allowlist() {
        let workspace = temp_workspace("target");
        let sandbox = LocalProcessSandbox::new("run-1", &workspace, std::env::temp_dir());
        let mutation = PreparedMutation {
            intent: MutationIntent {
                id: "mutation-1".into(),
                intent: "touch manifest".into(),
                target: MutationTarget::Paths {
                    allow: vec!["src".into()],
                },
                expected_effect: "should fail".into(),
                risk: RiskLevel::Low,
                signals: vec!["signal".into()],
                spec_id: None,
            },
            artifact: MutationArtifact {
                encoding: ArtifactEncoding::UnifiedDiff,
                payload: "\
diff --git a/Cargo.toml b/Cargo.toml
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/Cargo.toml
@@ -0,0 +1 @@
+[package]
"
                .into(),
                base_revision: Some("HEAD".into()),
                content_hash: "hash".into(),
            },
        };
        let result = sandbox.apply(&mutation, &SandboxPolicy::default()).await;
        assert!(matches!(result, Err(SandboxError::TargetViolation(_))));
    }

    #[test]
    fn should_skip_generated_artifact_roots() {
        assert!(should_skip(Path::new(".git")));
        assert!(should_skip(Path::new("target")));
        assert!(should_skip(Path::new("demo_runs")));
        assert!(!should_skip(Path::new("docs")));
    }
}

use std::env;
use std::error::Error;

use oris_orchestrator::coordinator::{Coordinator, CoordinatorConfig};

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("missing required environment variable: {}", name).into())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let runtime_base_url = required_env("RUNTIME_BASE_URL")?;
    let github_owner = required_env("GITHUB_OWNER")?;
    let github_repo = required_env("GITHUB_REPO")?;
    let github_token = required_env("GITHUB_TOKEN")?;

    let issue_id = env::var("ISSUE_ID").unwrap_or_else(|_| "issue-demo-1".to_string());
    let sender_id = env::var("A2A_SENDER_ID").unwrap_or_else(|_| "orchestrator-cli".to_string());
    let base_branch = env::var("BASE_BRANCH").unwrap_or_else(|_| "main".to_string());
    let branch_prefix = env::var("BRANCH_PREFIX").unwrap_or_else(|_| "codex".to_string());

    let config = CoordinatorConfig {
        sender_id,
        base_branch,
        branch_prefix,
    };

    let coordinator = Coordinator::with_http_clients(
        runtime_base_url,
        github_owner,
        github_repo,
        github_token,
        config,
    );

    let state = coordinator.run_single_issue(&issue_id).await?;
    println!("issue={} coordinator_state={}", issue_id, state.as_str());
    Ok(())
}

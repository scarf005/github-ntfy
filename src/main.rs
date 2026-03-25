mod config;
mod filter;
mod github;
mod ntfy;
mod render;
mod state;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::LoadedConfig;
use filter::{build_notification_facts, matching_block_rule};
use github::GitHubClient;
use ntfy::NtfyClient;
use render::render_notification;
use state::State;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "github-ntfy-agent")]
#[command(about = "Self-hosted GitHub notification agent for ntfy")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Once,
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let loaded = LoadedConfig::load(cli.config)?;
    init_tracing(&loaded.config.app.log_level)?;

    let github = GitHubClient::new(&loaded.config.github)?;
    let ntfy = NtfyClient::new(&loaded.config.ntfy)?;

    match cli.command {
        Command::Run => run_loop(&loaded, &github, &ntfy).await,
        Command::Once => {
            poll_once(&loaded, &github, &ntfy).await?;
            Ok(())
        }
        Command::Check => check(&loaded, &github, &ntfy).await,
    }
}

async fn run_loop(loaded: &LoadedConfig, github: &GitHubClient, ntfy: &NtfyClient) -> Result<()> {
    let mut backoff_secs = loaded.config.app.poll_interval_secs;

    loop {
        let next_sleep = match poll_once(loaded, github, ntfy).await {
            Ok(interval) => {
                backoff_secs = loaded.config.app.poll_interval_secs;
                interval
            }
            Err(error) => {
                error!(error = %error, "poll failed");
                backoff_secs = (backoff_secs.saturating_mul(2)).min(900);
                Duration::from_secs(backoff_secs)
            }
        };

        info!(
            sleep_secs = next_sleep.as_secs(),
            "sleeping before next poll"
        );
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received shutdown signal");
                return Ok(());
            }
            _ = tokio::time::sleep(next_sleep) => {}
        }
    }
}

async fn poll_once(
    loaded: &LoadedConfig,
    github: &GitHubClient,
    ntfy: &NtfyClient,
) -> Result<Duration> {
    let mut state = State::load(&loaded.state_path)?;
    let poll_result = github
        .poll_notifications(&loaded.config.github, state.last_modified.as_deref())
        .await?;

    if let Some(last_modified) = poll_result.last_modified {
        state.last_modified = Some(last_modified);
    }

    let mut sent_count = 0usize;
    for thread in poll_result
        .notifications
        .iter()
        .filter(|thread| thread.unread)
    {
        let dedupe_key = format!("{}|{}", thread.id, thread.updated_at);
        if state.has_seen(&dedupe_key) {
            continue;
        }

        let subject_kind = thread.subject.kind.as_deref();
        let should_enrich_timeline = matches!(subject_kind, Some("PullRequest"))
            && loaded.config.github.enrich_pull_requests
            || matches!(subject_kind, Some("Issue")) && loaded.config.github.enrich_issues;

        let (pull_request, timeline) = if let Some(subject_url) = thread.subject.url.as_deref() {
            let pull_request = if matches!(subject_kind, Some("PullRequest"))
                && loaded.config.github.enrich_pull_requests
            {
                github.pull_request_details(subject_url).await.ok()
            } else {
                None
            };
            let timeline = if should_enrich_timeline {
                github.issue_timeline(subject_url).await.ok()
            } else {
                None
            };
            (pull_request, timeline)
        } else {
            (None, None)
        };

        let rendered = render_notification(thread, pull_request.as_ref(), timeline.as_deref())?;
        let facts = build_notification_facts(thread, pull_request.as_ref(), timeline.as_deref());
        if let Some(rule) = matching_block_rule(&loaded.config.filters, &facts) {
            warn!(
                repo = %facts.repo_full_name,
                subject_type = %facts.subject_type,
                reason = %facts.reason,
                actor = facts.actor.as_deref().unwrap_or("unknown"),
                rule = rule.name.as_deref().unwrap_or("unnamed"),
                "notification blocked by rule"
            );
            state.mark_seen(rendered.dedupe_key, loaded.config.app.max_seen);
            continue;
        }
        ntfy.send(&rendered).await.with_context(|| {
            format!(
                "failed to send ntfy notification for {}",
                thread.repository.full_name
            )
        })?;
        state.mark_seen(rendered.dedupe_key, loaded.config.app.max_seen);
        sent_count += 1;
    }

    state.save(&loaded.state_path)?;
    info!(sent_count, "poll completed");

    let sleep_secs = poll_result
        .poll_interval_secs
        .unwrap_or(loaded.config.app.poll_interval_secs)
        .max(loaded.config.app.poll_interval_secs);
    Ok(Duration::from_secs(sleep_secs))
}

async fn check(loaded: &LoadedConfig, github: &GitHubClient, ntfy: &NtfyClient) -> Result<()> {
    github
        .check_notifications_access(&loaded.config.github)
        .await?;
    ntfy.check_access().await?;
    info!(
        config = %loaded.config_path.display(),
        state = %loaded.state_path.display(),
        "GitHub notifications API and ntfy endpoint are reachable"
    );
    Ok(())
}

fn init_tracing(default_level: &str) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .context("failed to configure logging")?;
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    Ok(())
}

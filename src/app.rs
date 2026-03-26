use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use crate::config::LoadedConfig;
use crate::filter::{build_notification_facts, matching_block_rule};
use crate::github::{GitHubClient, PullRequestDetails, Thread, TimelineEvent};
use crate::ntfy::NtfyClient;
use crate::render::render_notification;
use crate::state::State;

pub struct App {
    loaded: LoadedConfig,
    github: GitHubClient,
    ntfy: NtfyClient,
}

impl App {
    pub async fn new(loaded: LoadedConfig) -> Result<Self> {
        let github = GitHubClient::new(&loaded.config.github)?;
        let ntfy = NtfyClient::new(&loaded.config.ntfy)?;

        if loaded.config.github.token.is_none() {
            let login = github.current_user().await?;
            if let Some(login) = login {
                info!(github_login = %login, "using current gh auth session");
            }
        }

        Ok(Self {
            loaded,
            github,
            ntfy,
        })
    }

    pub async fn run_loop(&self) -> Result<()> {
        let mut backoff_secs = self.loaded.config.app.poll_interval_secs;

        loop {
            let next_sleep = match self.poll_once().await {
                Ok(interval) => {
                    backoff_secs = self.loaded.config.app.poll_interval_secs;
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

    pub async fn poll_once(&self) -> Result<Duration> {
        let mut state = State::load(&self.loaded.state_path)?;
        let poll_result = self
            .github
            .poll_notifications(&self.loaded.config.github, state.last_modified.as_deref())
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
            let rendered = self.process_thread(&mut state, thread).await?;
            sent_count += usize::from(rendered);
        }

        state.save(&self.loaded.state_path)?;
        info!(sent_count, "poll completed");

        let sleep_secs = poll_result
            .poll_interval_secs
            .unwrap_or(self.loaded.config.app.poll_interval_secs)
            .max(self.loaded.config.app.poll_interval_secs);
        Ok(Duration::from_secs(sleep_secs))
    }

    pub async fn check(&self) -> Result<()> {
        self.github
            .check_notifications_access(&self.loaded.config.github)
            .await?;
        self.ntfy.check_access().await?;
        info!(
            config = %self.loaded.config_path.display(),
            state = %self.loaded.state_path.display(),
            "GitHub notifications API and ntfy endpoint are reachable"
        );
        Ok(())
    }

    async fn process_thread(&self, state: &mut State, thread: &Thread) -> Result<bool> {
        let dedupe_key = format!("{}|{}", thread.id, thread.updated_at);
        if state.has_seen(&dedupe_key) {
            return Ok(false);
        }

        let (pull_request, timeline) = self.enrich_thread(thread).await;
        let rendered = render_notification(thread, pull_request.as_ref(), timeline.as_deref())?;
        let facts = build_notification_facts(thread, pull_request.as_ref(), timeline.as_deref());

        if let Some(rule) = matching_block_rule(&self.loaded.config.filters, &facts) {
            warn!(
                repo = %facts.repo_full_name,
                subject_type = %facts.subject_type,
                reason = %facts.reason,
                actor = facts.actor.as_deref().unwrap_or("unknown"),
                rule = rule.name.as_deref().unwrap_or("unnamed"),
                "notification blocked by rule"
            );
            state.mark_seen(rendered.dedupe_key, self.loaded.config.app.max_seen);
            return Ok(false);
        }

        self.ntfy.send(&rendered).await.with_context(|| {
            format!(
                "failed to send ntfy notification for {}",
                thread.repository.full_name
            )
        })?;
        state.mark_seen(rendered.dedupe_key, self.loaded.config.app.max_seen);
        Ok(true)
    }

    async fn enrich_thread(
        &self,
        thread: &Thread,
    ) -> (Option<PullRequestDetails>, Option<Vec<TimelineEvent>>) {
        let subject_kind = thread.subject.kind.as_deref();
        let should_enrich_pull_request = matches!(subject_kind, Some("PullRequest"))
            && self.loaded.config.github.enrich_pull_requests;
        let should_enrich_timeline = should_enrich_pull_request
            || matches!(subject_kind, Some("Issue")) && self.loaded.config.github.enrich_issues;

        let Some(subject_url) = thread.subject.url.as_deref() else {
            return (None, None);
        };

        let pull_request = if should_enrich_pull_request {
            match self.github.pull_request_details(subject_url).await {
                Ok(pull_request) => Some(pull_request),
                Err(error) => {
                    warn!(
                        error = %error,
                        repo = %thread.repository.full_name,
                        subject = %thread.subject.title.as_deref().unwrap_or("unknown"),
                        "failed to enrich pull request details"
                    );
                    None
                }
            }
        } else {
            None
        };

        let timeline = if should_enrich_timeline {
            match self.github.issue_timeline(subject_url).await {
                Ok(timeline) => Some(timeline),
                Err(error) => {
                    warn!(
                        error = %error,
                        repo = %thread.repository.full_name,
                        subject = %thread.subject.title.as_deref().unwrap_or("unknown"),
                        "failed to enrich issue timeline"
                    );
                    None
                }
            }
        } else {
            None
        };

        (pull_request, timeline)
    }
}

use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use crate::action;
use crate::auto_watch::should_watch_repository;
use crate::config::{BlockRule, LoadedConfig};
use crate::filter::{build_notification_facts, matching_block_rule};
use crate::github::{
    GitHubClient, PullRequestDetails, RepositorySubscriptionResult, Thread, TimelineEvent,
};
use crate::ntfy::NtfyClient;
use crate::render::{render_initial_subject_notification, render_notification};
use crate::state::{NotificationMerge, State};

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
        let action_server = if self.loaded.config.actions.enabled {
            Some(action::spawn_server(
                &self.loaded.config.actions,
                self.github.clone(),
            )?)
        } else {
            None
        };
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
                    if let Some(handle) = &action_server {
                        handle.abort();
                    }
                    info!("received shutdown signal");
                    return Ok(());
                }
                _ = tokio::time::sleep(next_sleep) => {}
            }
        }
    }

    pub async fn poll_once(&self) -> Result<Duration> {
        let mut state = State::load(&self.loaded.state_path)?;
        if let Err(error) = self.auto_watch_repositories(&mut state).await {
            warn!(error = %error, "auto-watch failed");
        }
        state.save(&self.loaded.state_path)?;
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

    async fn auto_watch_repositories(&self, state: &mut State) -> Result<()> {
        if !self.loaded.config.auto_watch.enabled {
            return Ok(());
        }

        let Some(current_user) = self.github.current_user().await? else {
            warn!("skipping auto-watch because current GitHub login could not be resolved");
            return Ok(());
        };

        let repositories = self
            .github
            .repositories_for_auto_watch(&self.loaded.config.github)
            .await?;
        let matching_repositories = repositories
            .iter()
            .filter(|repository| {
                should_watch_repository(&self.loaded.config.auto_watch, repository, &current_user)
            })
            .collect::<Vec<_>>();

        if !state.is_auto_watch_initialized() {
            let baselined_count = matching_repositories.len();
            for repository in matching_repositories {
                state.remember_auto_watched_repository(repository.full_name.clone());
            }
            state.mark_auto_watch_initialized();
            info!(
                baselined_count,
                "auto-watch baseline recorded without changing existing repository settings"
            );
            return Ok(());
        }

        let mut subscribed_count = 0usize;
        let mut skipped_count = 0usize;

        for repository in matching_repositories {
            if state.has_auto_watched_repository(&repository.full_name) {
                skipped_count += 1;
                continue;
            }

            match self
                .github
                .subscribe_repository(&repository.full_name)
                .await
            {
                Ok(RepositorySubscriptionResult::Subscribed) => {
                    state.remember_auto_watched_repository(repository.full_name.clone());
                    subscribed_count += 1;
                }
                Ok(RepositorySubscriptionResult::Skipped { reason }) => {
                    warn!(
                        reason,
                        repo = %repository.full_name,
                        "skipping repository subscription"
                    );
                    state.remember_auto_watched_repository(repository.full_name.clone());
                    skipped_count += 1;
                }
                Err(error) => warn!(
                    error = %error,
                    repo = %repository.full_name,
                    "failed to subscribe to repository"
                ),
            }
        }

        info!(subscribed_count, skipped_count, "auto-watch completed");
        Ok(())
    }

    async fn process_thread(&self, state: &mut State, thread: &Thread) -> Result<bool> {
        let dedupe_key = format!("{}|{}", thread.id, thread.updated_at);
        if state.has_seen(&dedupe_key) {
            return Ok(false);
        }

        let (pull_request, timeline) = self.enrich_thread(thread).await;
        let mut rendered = render_notification(thread, pull_request.as_ref(), timeline.as_deref())?;
        rendered.actions = action::notification_actions(&self.loaded.config.actions, &thread.id);
        let mut facts =
            build_notification_facts(thread, pull_request.as_ref(), timeline.as_deref());
        let mut merge = state.merge_notification(&rendered.sequence_id, &rendered.message);

        if matching_block_rule(&self.loaded.config.filters, &facts)
            .is_some_and(|rule| should_send_initial_subject_notification(rule, &merge, thread))
        {
            let body = self
                .initial_subject_body(thread, pull_request.as_ref())
                .await;
            rendered = render_initial_subject_notification(thread, body.as_deref())?;
            rendered.actions =
                action::notification_actions(&self.loaded.config.actions, &thread.id);
            facts = build_notification_facts(thread, pull_request.as_ref(), None);
            merge = state.merge_notification(&rendered.sequence_id, &rendered.message);
        }

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

        rendered.message = merge.message();

        if merge.had_existing && !merge.inserted_new_block {
            state.mark_seen(rendered.dedupe_key, self.loaded.config.app.max_seen);
            return Ok(false);
        }

        self.ntfy.send(&rendered).await.with_context(|| {
            format!(
                "failed to send ntfy notification for {}",
                thread.repository.full_name
            )
        })?;
        state.remember_notification(
            rendered.sequence_id.clone(),
            merge.blocks,
            self.loaded.config.app.max_seen,
        );
        state.mark_seen(rendered.dedupe_key, self.loaded.config.app.max_seen);
        Ok(true)
    }

    async fn initial_subject_body(
        &self,
        thread: &Thread,
        pull_request: Option<&PullRequestDetails>,
    ) -> Option<String> {
        if let Some(body) = pull_request.and_then(|pull_request| pull_request.body.clone()) {
            return Some(body);
        }

        if !matches!(
            thread.subject.kind.as_deref(),
            Some("PullRequest" | "Issue")
        ) {
            return None;
        }

        let subject_url = thread.subject.url.as_deref()?;
        match self.github.subject_details(subject_url).await {
            Ok(details) => details.body,
            Err(error) => {
                warn!(
                    error = %error,
                    repo = %thread.repository.full_name,
                    subject = %thread.subject.title.as_deref().unwrap_or("unknown"),
                    "failed to fetch initial subject body"
                );
                None
            }
        }
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

        if should_enrich_pull_request {
            match self.github.pull_request_enrichment(subject_url).await {
                Ok((pull_request, timeline)) => return (Some(pull_request), Some(timeline)),
                Err(error) => {
                    warn!(
                        error = %error,
                        repo = %thread.repository.full_name,
                        subject = %thread.subject.title.as_deref().unwrap_or("unknown"),
                        "failed to enrich pull request via GraphQL, falling back to REST"
                    );
                }
            }
        }

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

fn should_send_initial_subject_notification(
    rule: &BlockRule,
    merge: &NotificationMerge,
    thread: &Thread,
) -> bool {
    rule.activity.is_some()
        && !merge.had_existing
        && matches!(
            thread.subject.kind.as_deref(),
            Some("PullRequest" | "Issue")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{Repository, Subject};

    fn thread(kind: &str) -> Thread {
        Thread {
            id: String::from("1"),
            unread: true,
            updated_at: String::from("2026-06-15T00:07:51Z"),
            reason: Some(String::from("subscribed")),
            repository: Repository {
                full_name: String::from("cataclysmbn/Cataclysm-BN"),
                html_url: String::from("https://github.com/cataclysmbn/Cataclysm-BN"),
                owner: None,
            },
            subject: Subject {
                title: Some(String::from("feat: allow pens to be used as writing tools")),
                kind: Some(String::from(kind)),
                url: Some(String::from(
                    "https://api.github.com/repos/cataclysmbn/Cataclysm-BN/pulls/9503",
                )),
            },
        }
    }

    fn merge(had_existing: bool) -> NotificationMerge {
        NotificationMerge {
            blocks: vec![String::from("Added label: JSON")],
            had_existing,
            inserted_new_block: true,
        }
    }

    #[test]
    fn sends_initial_subject_when_first_activity_is_filtered() {
        let rule = BlockRule {
            activity: Some(String::from("labeled")),
            ..BlockRule::default()
        };

        assert!(should_send_initial_subject_notification(
            &rule,
            &merge(false),
            &thread("PullRequest")
        ));
    }

    #[test]
    fn keeps_filtering_activity_after_subject_was_sent() {
        let rule = BlockRule {
            activity: Some(String::from("labeled")),
            ..BlockRule::default()
        };

        assert!(!should_send_initial_subject_notification(
            &rule,
            &merge(true),
            &thread("PullRequest")
        ));
    }

    #[test]
    fn does_not_bypass_non_activity_filters() {
        let rule = BlockRule {
            repo: Some(String::from("cataclysmbn/Cataclysm-BN")),
            ..BlockRule::default()
        };

        assert!(!should_send_initial_subject_notification(
            &rule,
            &merge(false),
            &thread("PullRequest")
        ));
    }
}

use anyhow::{Context, Result};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED,
    USER_AGENT,
};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use std::process::Command;

use crate::config::GitHubConfig;

const API_VERSION_HEADER: HeaderName = HeaderName::from_static("x-github-api-version");
const POLL_INTERVAL_HEADER: HeaderName = HeaderName::from_static("x-poll-interval");

#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    api_base: Url,
    token: String,
}

pub struct PollResult {
    pub notifications: Vec<Thread>,
    pub last_modified: Option<String>,
    pub poll_interval_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thread {
    pub id: String,
    pub unread: bool,
    pub updated_at: String,
    pub reason: Option<String>,
    pub repository: Repository,
    pub subject: Subject,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    pub full_name: String,
    pub html_url: String,
    pub owner: Option<Owner>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Owner {
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Subject {
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestDetails {
    #[serde(default)]
    pub merged: bool,
    pub merged_by: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimelineEvent {
    pub event: Option<String>,
    pub actor: Option<User>,
    pub user: Option<User>,
    pub author: Option<User>,
    pub committer: Option<User>,
    pub assignee: Option<User>,
    pub review_requester: Option<User>,
    pub requested_reviewer: Option<User>,
    pub requested_team: Option<Team>,
    pub label: Option<Label>,
    pub dismissed_review: Option<DismissedReview>,
    pub body: Option<String>,
    pub message: Option<String>,
    pub commit: Option<Commit>,
    pub state: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub login: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Team {
    pub slug: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DismissedReview {
    pub dismissal_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Commit {
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TimelineActivity {
    pub kind: String,
    pub actor: String,
    pub actor_is_bot: bool,
    pub detail: Option<String>,
    pub commit_count: Option<usize>,
}

impl GitHubClient {
    pub fn new(config: &GitHubConfig) -> Result<Self> {
        let client = Client::builder()
            .build()
            .context("failed to build GitHub client")?;
        let api_base = Url::parse(&config.api_base).context("invalid github.api_base")?;
        let token = resolve_token(config.token.as_deref())?;

        Ok(Self {
            client,
            api_base,
            token,
        })
    }

    pub async fn poll_notifications(
        &self,
        config: &GitHubConfig,
        last_modified: Option<&str>,
    ) -> Result<PollResult> {
        let mut request = self
            .client
            .get(self.endpoint("/notifications")?)
            .headers(self.headers()?);
        request = request.query(&[
            ("all", "false"),
            (
                "participating",
                if config.participating {
                    "true"
                } else {
                    "false"
                },
            ),
            ("page", "1"),
        ]);
        if let Some(last_modified) = last_modified {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }
        let request = request.query(&[
            ("per_page", config.per_page.to_string()),
            ("page", String::from("1")),
        ]);
        let response = request
            .send()
            .await
            .context("failed to poll GitHub notifications")?;

        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(PollResult {
                notifications: Vec::new(),
                last_modified: last_modified.map(String::from),
                poll_interval_secs: parse_poll_interval(response.headers()),
            });
        }

        let response = response
            .error_for_status()
            .context("GitHub notifications request failed")?;
        let headers = response.headers().clone();
        let notifications = response
            .json::<Vec<Thread>>()
            .await
            .context("failed to decode GitHub notifications")?;

        Ok(PollResult {
            notifications,
            last_modified: headers
                .get(LAST_MODIFIED)
                .and_then(|value| value.to_str().ok())
                .map(String::from),
            poll_interval_secs: parse_poll_interval(&headers),
        })
    }

    pub async fn pull_request_details(&self, subject_url: &str) -> Result<PullRequestDetails> {
        self.client
            .get(subject_url)
            .headers(self.headers()?)
            .send()
            .await
            .context("failed to fetch pull request details")?
            .error_for_status()
            .context("pull request details request failed")?
            .json::<PullRequestDetails>()
            .await
            .context("failed to decode pull request details")
    }

    pub async fn issue_timeline(&self, subject_url: &str) -> Result<Vec<TimelineEvent>> {
        let timeline_url = timeline_url(subject_url)?;
        self.client
            .get(timeline_url)
            .headers(self.headers()?)
            .send()
            .await
            .context("failed to fetch issue timeline")?
            .error_for_status()
            .context("issue timeline request failed")?
            .json::<Vec<TimelineEvent>>()
            .await
            .context("failed to decode issue timeline")
    }

    pub async fn check_notifications_access(&self, config: &GitHubConfig) -> Result<()> {
        let _ = self.poll_notifications(config, None).await?;
        Ok(())
    }

    pub async fn current_user(&self) -> Result<Option<String>> {
        let response = self
            .client
            .get(self.endpoint("/user")?)
            .headers(self.headers()?)
            .send()
            .await
            .context("failed to reach GitHub user endpoint")?;

        if response.status() == StatusCode::UNAUTHORIZED {
            return Ok(None);
        }

        if !response.status().is_success() {
            response
                .error_for_status()
                .context("GitHub user endpoint returned an error")?;
            return Ok(None);
        }

        #[derive(Deserialize)]
        struct UserInfo {
            login: Option<String>,
        }

        let info = response
            .json::<UserInfo>()
            .await
            .context("failed to decode GitHub user response")?;
        Ok(info.login)
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("github-ntfy-agent"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(API_VERSION_HEADER, HeaderValue::from_static("2022-11-28"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.token))
                .context("invalid GitHub token")?,
        );
        Ok(headers)
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.api_base
            .join(path.trim_start_matches('/'))
            .with_context(|| format!("failed to build GitHub endpoint for {path}"))
    }
}

impl TimelineEvent {
    fn event_time(&self) -> &str {
        self.submitted_at
            .as_deref()
            .or(self.updated_at.as_deref())
            .or(self.created_at.as_deref())
            .unwrap_or("")
    }

    fn actor_name(&self) -> String {
        self.user
            .as_ref()
            .or(self.actor.as_ref())
            .or(self.author.as_ref())
            .or(self.committer.as_ref())
            .map(|user| user.login.clone())
            .unwrap_or_else(|| String::from("someone"))
    }

    fn actor_is_bot(&self) -> bool {
        self.user
            .as_ref()
            .or(self.actor.as_ref())
            .or(self.author.as_ref())
            .or(self.committer.as_ref())
            .is_some_and(User::is_bot)
    }

    fn detail_text(&self) -> Option<String> {
        self.body.clone().or_else(|| {
            self.dismissed_review
                .as_ref()
                .and_then(|review| review.dismissal_message.clone())
        })
    }

    fn cleaned_commit_message(&self) -> Option<String> {
        let message = self.message.as_deref().or_else(|| {
            self.commit
                .as_ref()
                .and_then(|commit| commit.message.as_deref())
        })?;
        let first_line = message
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())?;
        Some(String::from(first_line))
    }
}

impl TimelineActivity {
    pub fn from_timeline(timeline: &[TimelineEvent]) -> Option<Self> {
        let mut events: Vec<_> = timeline
            .iter()
            .filter(|event| {
                matches!(
                    event.event.as_deref(),
                    Some(
                        "reviewed"
                            | "commented"
                            | "committed"
                            | "merged"
                            | "assigned"
                            | "unassigned"
                            | "labeled"
                            | "unlabeled"
                            | "review_requested"
                            | "review_request_removed"
                            | "review_dismissed"
                            | "closed"
                            | "reopened"
                            | "ready_for_review"
                            | "convert_to_draft"
                    )
                )
            })
            .collect();
        events.sort_by_key(|event| event.event_time().to_string());

        let last = events.last()?;
        let actor = last.actor_name();
        let actor_is_bot = last.actor_is_bot();

        match last.event.as_deref()? {
            "reviewed" => Some(Self {
                kind: match last
                    .state
                    .as_deref()
                    .map(|state| state.to_ascii_uppercase())
                {
                    Some(state) if state == "APPROVED" => String::from("review_approved"),
                    Some(state) if state == "CHANGES_REQUESTED" => {
                        String::from("review_changes_requested")
                    }
                    _ => String::from("reviewed"),
                },
                actor,
                actor_is_bot,
                detail: last.detail_text(),
                commit_count: None,
            }),
            "commented" => Some(Self {
                kind: String::from("commented"),
                actor,
                actor_is_bot,
                detail: last.detail_text(),
                commit_count: None,
            }),
            "committed" => {
                let mut grouped = vec![*last];
                for event in events.iter().rev().skip(1) {
                    if event.event.as_deref() == Some("committed") && event.actor_name() == actor {
                        grouped.push(*event);
                    } else {
                        break;
                    }
                }
                grouped.reverse();
                let detail = grouped
                    .iter()
                    .filter_map(|event| event.cleaned_commit_message())
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("\n");

                Some(Self {
                    kind: String::from("committed"),
                    actor,
                    actor_is_bot,
                    detail: (!detail.is_empty()).then_some(detail),
                    commit_count: Some(grouped.len()),
                })
            }
            "merged" => Some(Self {
                kind: String::from("merged"),
                actor,
                actor_is_bot,
                detail: last.detail_text(),
                commit_count: None,
            }),
            "assigned" => Some(Self {
                kind: String::from("assigned"),
                actor,
                actor_is_bot,
                detail: Some(format!(
                    "Assigned to @{}",
                    last.assignee
                        .as_ref()
                        .map(|assignee| assignee.login.as_str())
                        .unwrap_or("someone")
                )),
                commit_count: None,
            }),
            "unassigned" => Some(Self {
                kind: String::from("unassigned"),
                actor,
                actor_is_bot,
                detail: Some(format!(
                    "Unassigned from @{}",
                    last.assignee
                        .as_ref()
                        .map(|assignee| assignee.login.as_str())
                        .unwrap_or("someone")
                )),
                commit_count: None,
            }),
            "labeled" => Some(Self {
                kind: String::from("labeled"),
                actor,
                actor_is_bot,
                detail: Some(format!(
                    "Added label: {}",
                    last.label
                        .as_ref()
                        .map(|label| label.name.as_str())
                        .unwrap_or("unknown")
                )),
                commit_count: None,
            }),
            "unlabeled" => Some(Self {
                kind: String::from("unlabeled"),
                actor,
                actor_is_bot,
                detail: Some(format!(
                    "Removed label: {}",
                    last.label
                        .as_ref()
                        .map(|label| label.name.as_str())
                        .unwrap_or("unknown")
                )),
                commit_count: None,
            }),
            "review_requested" => Some(Self {
                kind: String::from("review_requested"),
                actor: last
                    .review_requester
                    .as_ref()
                    .map(|reviewer| reviewer.login.clone())
                    .unwrap_or(actor),
                actor_is_bot: last.review_requester.as_ref().is_some_and(User::is_bot)
                    || actor_is_bot,
                detail: Some(format!(
                    "Requested from @{}",
                    last.requested_reviewer
                        .as_ref()
                        .map(|reviewer| reviewer.login.as_str())
                        .or_else(|| last.requested_team.as_ref().map(|team| team.slug.as_str()))
                        .unwrap_or("someone")
                )),
                commit_count: None,
            }),
            "review_request_removed" => Some(Self {
                kind: String::from("review_request_removed"),
                actor: last
                    .review_requester
                    .as_ref()
                    .map(|reviewer| reviewer.login.clone())
                    .unwrap_or(actor),
                actor_is_bot: last.review_requester.as_ref().is_some_and(User::is_bot)
                    || actor_is_bot,
                detail: Some(format!(
                    "Removed for @{}",
                    last.requested_reviewer
                        .as_ref()
                        .map(|reviewer| reviewer.login.as_str())
                        .or_else(|| last.requested_team.as_ref().map(|team| team.slug.as_str()))
                        .unwrap_or("someone")
                )),
                commit_count: None,
            }),
            kind => Some(Self {
                kind: String::from(kind),
                actor,
                actor_is_bot,
                detail: last.detail_text(),
                commit_count: None,
            }),
        }
    }
}

impl User {
    pub fn is_bot(&self) -> bool {
        self.kind.as_deref() == Some("Bot") || self.login.ends_with("[bot]")
    }
}

fn timeline_url(subject_url: &str) -> Result<String> {
    if subject_url.contains("/pulls/") {
        return Ok(subject_url.replace("/pulls/", "/issues/") + "/timeline?per_page=100");
    }

    if subject_url.contains("/issues/") {
        return Ok(format!("{subject_url}/timeline?per_page=100"));
    }

    anyhow::bail!("unsupported subject URL for timeline: {subject_url}")
}

fn parse_poll_interval(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(POLL_INTERVAL_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn resolve_token(configured_token: Option<&str>) -> Result<String> {
    if let Some(token) = configured_token.filter(|token| !token.trim().is_empty()) {
        return Ok(token.to_string());
    }

    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("failed to run `gh auth token`")?;

    if !output.status.success() {
        anyhow::bail!(
            "`gh auth token` failed; set github.token in config or login with `gh auth login`"
        );
    }

    let token =
        String::from_utf8(output.stdout).context("`gh auth token` returned invalid utf-8")?;
    let token = token.trim();
    if token.is_empty() {
        anyhow::bail!(
            "`gh auth token` returned an empty token; set github.token in config or login with `gh auth login`"
        );
    }

    Ok(token.to_string())
}

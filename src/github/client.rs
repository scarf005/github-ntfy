use std::time::Duration;

use anyhow::{Context, Result};
use graphql_client::{GraphQLQuery, Response};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED, USER_AGENT,
};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use serde_json::json;

use crate::config::GitHubConfig;

use super::auth::resolve_token;
use super::model::{
    AutoWatchRepository, PollResult, PullRequestDetails, RepositorySubscriptionResult,
    SubjectDetails, Thread, TimelineEvent,
};
use super::timeline::timeline_url;

type DateTime = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github/graphql/github-schema.graphql",
    query_path = "src/github/graphql/pull_request_enrichment.graphql",
    response_derives = "Debug, Clone"
)]
struct PullRequestEnrichment;

use pull_request_enrichment as gql;

const API_VERSION_HEADER: HeaderName = HeaderName::from_static("x-github-api-version");
const POLL_INTERVAL_HEADER: HeaderName = HeaderName::from_static("x-poll-interval");

type GraphQlPullRequest = gql::PullRequestEnrichmentRepositoryPullRequest;
type GraphQlTimelineItem = gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodes;
type GraphQlReviewComment =
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnPullRequestReviewCommentsNodes;
type GraphQlReviewCommentConnection =
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnPullRequestReviewComments;
type GraphQlCommit = gql::PullRequestEnrichmentRepositoryPullRequestCommitsNodesCommit;

#[derive(Debug)]
struct PullRequestSubject {
    owner: String,
    repo: String,
    number: i64,
}

#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    api_base: Url,
    token: String,
}

impl GitHubClient {
    pub fn new(config: &GitHubConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
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

    pub async fn repositories_for_auto_watch(
        &self,
        config: &GitHubConfig,
    ) -> Result<Vec<AutoWatchRepository>> {
        let per_page = config.per_page.clamp(1, 100);
        let mut page = 1u32;
        let mut repositories = Vec::new();

        loop {
            let batch = self
                .client
                .get(self.endpoint("/user/repos")?)
                .headers(self.headers()?)
                .query(&[
                    (
                        "affiliation",
                        String::from("owner,collaborator,organization_member"),
                    ),
                    ("sort", String::from("created")),
                    ("direction", String::from("desc")),
                    ("per_page", per_page.to_string()),
                    ("page", page.to_string()),
                ])
                .send()
                .await
                .context("failed to list GitHub repositories for auto-watch")?
                .error_for_status()
                .context("GitHub repository list request failed")?
                .json::<Vec<AutoWatchRepository>>()
                .await
                .context("failed to decode GitHub repositories")?;

            let batch_len = batch.len();
            repositories.extend(batch);

            if batch_len < per_page as usize {
                break;
            }
            page = page.saturating_add(1);
        }

        Ok(repositories)
    }

    pub async fn subscribe_repository(
        &self,
        full_name: &str,
    ) -> Result<RepositorySubscriptionResult> {
        let existing = self
            .client
            .get(self.endpoint(&format!("/repos/{full_name}/subscription"))?)
            .headers(self.headers()?)
            .send()
            .await
            .with_context(|| format!("failed to check repository subscription {full_name}"))?;

        match existing.status() {
            StatusCode::OK => {
                return Ok(RepositorySubscriptionResult::Skipped {
                    reason: String::from("repository already has an explicit subscription setting"),
                });
            }
            StatusCode::NOT_FOUND => {}
            status => {
                let body = existing.text().await.unwrap_or_default();
                return skipped_or_error(full_name, status, body);
            }
        }

        let response = self
            .client
            .put(self.endpoint(&format!("/repos/{full_name}/subscription"))?)
            .headers(self.headers()?)
            .json(&json!({ "subscribed": true, "ignored": false }))
            .send()
            .await
            .with_context(|| format!("failed to subscribe to repository {full_name}"))?;

        if response.status().is_success() {
            return Ok(RepositorySubscriptionResult::Subscribed);
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        skipped_or_error(full_name, status, body)
    }

    pub async fn poll_notifications(
        &self,
        config: &GitHubConfig,
        _last_modified: Option<&str>,
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
                last_modified: None,
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

    pub async fn pull_request_enrichment(
        &self,
        subject_url: &str,
    ) -> Result<(PullRequestDetails, Vec<TimelineEvent>)> {
        let subject = parse_pull_request_subject(subject_url)?;
        let response = self
            .client
            .post(self.endpoint("/graphql")?)
            .headers(self.headers()?)
            .json(&PullRequestEnrichment::build_query(
                pull_request_enrichment::Variables {
                    owner: subject.owner,
                    name: subject.repo,
                    number: subject.number,
                },
            ))
            .send()
            .await
            .context("failed to fetch pull request enrichment")?
            .error_for_status()
            .context("pull request enrichment request failed")?
            .json::<Response<pull_request_enrichment::ResponseData>>()
            .await
            .context("failed to decode pull request enrichment")?;

        if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
            anyhow::bail!(
                "pull request enrichment query failed: {}",
                errors
                    .iter()
                    .map(|error| error.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }

        let pull_request = response
            .data
            .and_then(|data| data.repository)
            .and_then(|repository| repository.pull_request)
            .context("pull request enrichment returned no pull request")?;

        Ok((
            PullRequestDetails {
                merged: pull_request.merged,
                merged_by: pull_request.merged_by.as_ref().map(graphql_actor_to_user),
                body: Some(pull_request.body.clone()),
            },
            graphql_timeline_to_rest(&pull_request),
        ))
    }

    pub async fn subject_details(&self, subject_url: &str) -> Result<SubjectDetails> {
        self.client
            .get(subject_url)
            .headers(self.headers()?)
            .send()
            .await
            .context("failed to fetch subject details")?
            .error_for_status()
            .context("subject details request failed")?
            .json::<SubjectDetails>()
            .await
            .context("failed to decode subject details")
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

    pub async fn mark_thread_as_read(&self, thread_id: &str) -> Result<()> {
        self.client
            .patch(self.endpoint(&format!("/notifications/threads/{thread_id}"))?)
            .headers(self.headers()?)
            .send()
            .await
            .context("failed to mark thread as read")?
            .error_for_status()
            .context("mark thread as read request failed")?;
        Ok(())
    }

    pub async fn mark_thread_as_done(&self, thread_id: &str) -> Result<()> {
        self.client
            .delete(self.endpoint(&format!("/notifications/threads/{thread_id}"))?)
            .headers(self.headers()?)
            .send()
            .await
            .context("failed to mark thread as done")?
            .error_for_status()
            .context("mark thread as done request failed")?;
        Ok(())
    }

    pub async fn ignore_thread(&self, thread_id: &str) -> Result<()> {
        self.client
            .put(self.endpoint(&format!("/notifications/threads/{thread_id}/subscription"))?)
            .headers(self.headers()?)
            .json(&json!({ "ignored": true }))
            .send()
            .await
            .context("failed to ignore thread")?
            .error_for_status()
            .context("ignore thread request failed")?;
        Ok(())
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

fn skipped_or_error(
    full_name: &str,
    status: StatusCode,
    body: String,
) -> Result<RepositorySubscriptionResult> {
    if status == StatusCode::FORBIDDEN && body.contains("Repository access blocked") {
        return Ok(RepositorySubscriptionResult::Skipped {
            reason: format!("{status}: {body}"),
        });
    }

    anyhow::bail!("repository subscription request failed for {full_name}: {status}: {body}");
}

fn parse_poll_interval(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(POLL_INTERVAL_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn parse_pull_request_subject(subject_url: &str) -> Result<PullRequestSubject> {
    let url = Url::parse(subject_url)
        .with_context(|| format!("failed to parse pull request subject URL: {subject_url}"))?;
    let segments = url
        .path_segments()
        .context("pull request subject URL has no path segments")?
        .collect::<Vec<_>>();

    let ["repos", owner, repo, "pulls", number] = segments.as_slice() else {
        anyhow::bail!("unsupported pull request subject URL: {subject_url}");
    };

    Ok(PullRequestSubject {
        owner: String::from(*owner),
        repo: String::from(*repo),
        number: number
            .parse::<i64>()
            .with_context(|| format!("invalid pull request number in {subject_url}"))?,
    })
}

fn graphql_timeline_to_rest(pull_request: &GraphQlPullRequest) -> Vec<TimelineEvent> {
    let mut timeline = pull_request
        .timeline_items
        .nodes
        .iter()
        .flatten()
        .map(graphql_timeline_event)
        .collect::<Vec<_>>();

    timeline.extend(
        pull_request
            .commits
            .nodes
            .iter()
            .flatten()
            .map(|node| graphql_commit_event(&node.commit)),
    );

    timeline
}

fn graphql_timeline_event(item: &GraphQlTimelineItem) -> TimelineEvent {
    match item {
        GraphQlTimelineItem::PullRequestReview(review) => {
            let latest_comment = latest_review_comment(&review.comments);
            let actor = if let Some(author) = review.author.as_ref() {
                Some(graphql_actor_to_user(author))
            } else {
                latest_comment
                    .and_then(|comment| comment.author.as_ref())
                    .map(graphql_actor_to_user)
            };
            TimelineEvent {
                event: Some(String::from("reviewed")),
                actor,
                body: review_detail(&review.body, latest_comment),
                state: Some(review_state(&review.state)),
                created_at: Some(review.created_at.clone()),
                submitted_at: Some(review.created_at.clone()),
                ..TimelineEvent::default()
            }
        }
        GraphQlTimelineItem::IssueComment(comment) => TimelineEvent {
            event: Some(String::from("commented")),
            actor: comment.author.as_ref().map(graphql_actor_to_user),
            body: Some(comment.body.clone()),
            created_at: Some(comment.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::ReviewRequestedEvent(event) => TimelineEvent {
            event: Some(String::from("review_requested")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            review_requester: event.actor.as_ref().map(graphql_actor_to_user),
            requested_reviewer: event
                .requested_reviewer
                .as_ref()
                .and_then(review_requested_reviewer_user),
            requested_team: event
                .requested_reviewer
                .as_ref()
                .and_then(review_requested_reviewer_team),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::ReviewRequestRemovedEvent(event) => TimelineEvent {
            event: Some(String::from("review_request_removed")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            review_requester: event.actor.as_ref().map(graphql_actor_to_user),
            requested_reviewer: event
                .requested_reviewer
                .as_ref()
                .and_then(review_request_removed_reviewer_user),
            requested_team: event
                .requested_reviewer
                .as_ref()
                .and_then(review_request_removed_reviewer_team),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::ReviewDismissedEvent(event) => TimelineEvent {
            event: Some(String::from("review_dismissed")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            dismissed_review: Some(super::model::DismissedReview {
                dismissal_message: event.dismissal_message.clone(),
            }),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::MergedEvent(event) => simple_timeline_event(
            "merged",
            event.actor.as_ref().map(graphql_actor_to_user),
            Some(event.created_at.clone()),
        ),
        GraphQlTimelineItem::ClosedEvent(event) => simple_timeline_event(
            "closed",
            event.actor.as_ref().map(graphql_actor_to_user),
            Some(event.created_at.clone()),
        ),
        GraphQlTimelineItem::ReopenedEvent(event) => simple_timeline_event(
            "reopened",
            event.actor.as_ref().map(graphql_actor_to_user),
            Some(event.created_at.clone()),
        ),
        GraphQlTimelineItem::ReadyForReviewEvent(event) => simple_timeline_event(
            "ready_for_review",
            event.actor.as_ref().map(graphql_actor_to_user),
            Some(event.created_at.clone()),
        ),
        GraphQlTimelineItem::ConvertToDraftEvent(event) => simple_timeline_event(
            "convert_to_draft",
            event.actor.as_ref().map(graphql_actor_to_user),
            Some(event.created_at.clone()),
        ),
        GraphQlTimelineItem::LabeledEvent(event) => TimelineEvent {
            event: Some(String::from("labeled")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            label: event.label.as_ref().map(|label| super::model::Label {
                name: label.name.clone(),
            }),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::UnlabeledEvent(event) => TimelineEvent {
            event: Some(String::from("unlabeled")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            label: event.label.as_ref().map(|label| super::model::Label {
                name: label.name.clone(),
            }),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::AssignedEvent(event) => TimelineEvent {
            event: Some(String::from("assigned")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            assignee: event.assignee.as_ref().map(assigned_assignee_to_user),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
        GraphQlTimelineItem::UnassignedEvent(event) => TimelineEvent {
            event: Some(String::from("unassigned")),
            actor: event.actor.as_ref().map(graphql_actor_to_user),
            assignee: event.assignee.as_ref().map(unassigned_assignee_to_user),
            created_at: Some(event.created_at.clone()),
            ..TimelineEvent::default()
        },
    }
}

fn review_state(state: &gql::PullRequestReviewState) -> String {
    match state {
        gql::PullRequestReviewState::APPROVED => String::from("APPROVED"),
        gql::PullRequestReviewState::CHANGES_REQUESTED => String::from("CHANGES_REQUESTED"),
        gql::PullRequestReviewState::COMMENTED => String::from("COMMENTED"),
        gql::PullRequestReviewState::DISMISSED => String::from("DISMISSED"),
        gql::PullRequestReviewState::PENDING => String::from("PENDING"),
        gql::PullRequestReviewState::Other(state) => state.clone(),
    }
}

fn review_detail(body: &str, fallback_comment: Option<&GraphQlReviewComment>) -> Option<String> {
    (!body.trim().is_empty())
        .then(|| String::from(body))
        .or_else(|| fallback_comment.map(|comment| comment.body.clone()))
}

fn latest_review_comment(
    comments: &GraphQlReviewCommentConnection,
) -> Option<&GraphQlReviewComment> {
    comments
        .nodes
        .iter()
        .flatten()
        .filter(|comment| !comment.body.trim().is_empty())
        .max_by_key(|comment| comment.created_at.as_str())
}

fn graphql_commit_event(commit: &GraphQlCommit) -> TimelineEvent {
    let actor = commit
        .author
        .as_ref()
        .and_then(commit_signature_to_user)
        .or_else(|| commit.committer.as_ref().and_then(commit_signature_to_user));

    TimelineEvent {
        event: Some(String::from("committed")),
        author: actor,
        message: Some(commit.message_headline.clone()),
        created_at: Some(commit.authored_date.clone()),
        ..TimelineEvent::default()
    }
}

fn simple_timeline_event(
    event: &str,
    actor: Option<super::model::User>,
    created_at: Option<String>,
) -> TimelineEvent {
    TimelineEvent {
        event: Some(String::from(event)),
        actor,
        created_at,
        ..TimelineEvent::default()
    }
}

trait GeneratedActor {
    fn login(&self) -> &str;
    fn kind(&self) -> &'static str;
}

macro_rules! impl_generated_actor {
    ($actor:ty, $on:ty) => {
        impl GeneratedActor for $actor {
            fn login(&self) -> &str {
                &self.login
            }

            fn kind(&self) -> &'static str {
                match &self.on {
                    <$on>::Bot => "Bot",
                    <$on>::EnterpriseUserAccount => "EnterpriseUserAccount",
                    <$on>::Mannequin => "Mannequin",
                    <$on>::Organization => "Organization",
                    <$on>::User => "User",
                }
            }
        }
    };
}

impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestMergedBy,
    gql::PullRequestEnrichmentRepositoryPullRequestMergedByOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnPullRequestReviewAuthor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnPullRequestReviewAuthorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnPullRequestReviewCommentsNodesAuthor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnPullRequestReviewCommentsNodesAuthorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnIssueCommentAuthor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnIssueCommentAuthorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewDismissedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewDismissedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnMergedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnMergedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnClosedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnClosedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReopenedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReopenedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReadyForReviewEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReadyForReviewEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnConvertToDraftEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnConvertToDraftEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnLabeledEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnLabeledEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnlabeledEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnlabeledEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventActorOn
);
impl_generated_actor!(
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventActor,
    gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventActorOn
);
fn graphql_actor_to_user(actor: &impl GeneratedActor) -> super::model::User {
    super::model::User {
        login: String::from(actor.login()),
        kind: Some(String::from(actor.kind())),
    }
}

fn assigned_assignee_to_user(
    assignee: &gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventAssignee,
) -> super::model::User {
    match assignee {
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventAssignee::Bot(bot) => {
            super::model::User {
                login: bot.login.clone(),
                kind: Some(String::from("Bot")),
            }
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventAssignee::Mannequin(user) => {
            super::model::User {
                login: user.login.clone(),
                kind: Some(String::from("Mannequin")),
            }
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventAssignee::Organization(org) => {
            super::model::User {
                login: org.login.clone(),
                kind: Some(String::from("Organization")),
            }
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnAssignedEventAssignee::User(user) => {
            super::model::User {
                login: user.login.clone(),
                kind: Some(String::from("User")),
            }
        }
    }
}

fn unassigned_assignee_to_user(
    assignee: &gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventAssignee,
) -> super::model::User {
    match assignee {
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventAssignee::Bot(bot) => {
            super::model::User {
                login: bot.login.clone(),
                kind: Some(String::from("Bot")),
            }
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventAssignee::Mannequin(user) => {
            super::model::User {
                login: user.login.clone(),
                kind: Some(String::from("Mannequin")),
            }
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventAssignee::Organization(org) => {
            super::model::User {
                login: org.login.clone(),
                kind: Some(String::from("Organization")),
            }
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnUnassignedEventAssignee::User(user) => {
            super::model::User {
                login: user.login.clone(),
                kind: Some(String::from("User")),
            }
        }
    }
}

fn review_requested_reviewer_user(
    reviewer: &gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer,
) -> Option<super::model::User> {
    match reviewer {
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer::User(user) => Some(super::model::User {
            login: user.login.clone(),
            kind: None,
        }),
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer::Bot(bot) => Some(super::model::User {
            login: bot.login.clone(),
            kind: Some(String::from("Bot")),
        }),
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer::Team(_) => None,
    }
}

fn review_requested_reviewer_team(
    reviewer: &gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer,
) -> Option<super::model::Team> {
    match reviewer {
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer::Team(team) => {
            Some(super::model::Team { slug: team.slug.clone() })
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer::User(_)
        | gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestedEventRequestedReviewer::Bot(_) => None,
    }
}

fn review_request_removed_reviewer_user(
    reviewer: &gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer,
) -> Option<super::model::User> {
    match reviewer {
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer::User(user) => Some(super::model::User {
            login: user.login.clone(),
            kind: None,
        }),
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer::Bot(bot) => Some(super::model::User {
            login: bot.login.clone(),
            kind: Some(String::from("Bot")),
        }),
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer::Team(_) => None,
    }
}

fn review_request_removed_reviewer_team(
    reviewer: &gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer,
) -> Option<super::model::Team> {
    match reviewer {
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer::Team(team) => {
            Some(super::model::Team { slug: team.slug.clone() })
        }
        gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer::User(_)
        | gql::PullRequestEnrichmentRepositoryPullRequestTimelineItemsNodesOnReviewRequestRemovedEventRequestedReviewer::Bot(_) => None,
    }
}

trait CommitSignature {
    fn name(&self) -> Option<&str>;
    fn user_login(&self) -> Option<&str>;
}

impl CommitSignature for gql::PullRequestEnrichmentRepositoryPullRequestCommitsNodesCommitAuthor {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn user_login(&self) -> Option<&str> {
        self.user.as_ref().map(|user| user.login.as_str())
    }
}

impl CommitSignature
    for gql::PullRequestEnrichmentRepositoryPullRequestCommitsNodesCommitCommitter
{
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn user_login(&self) -> Option<&str> {
        self.user.as_ref().map(|user| user.login.as_str())
    }
}

fn commit_signature_to_user(signature: &impl CommitSignature) -> Option<super::model::User> {
    let login = signature
        .user_login()
        .map(String::from)
        .or_else(|| signature.name().map(String::from))?;

    Some(super::model::User {
        kind: login.ends_with("[bot]").then(|| String::from("Bot")),
        login,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pull_request_subject_url() {
        let subject = parse_pull_request_subject(
            "https://api.github.com/repos/cataclysmbn/Cataclysm-BN/pulls/8404",
        )
        .expect("subject");

        assert_eq!(subject.owner, "cataclysmbn");
        assert_eq!(subject.repo, "Cataclysm-BN");
        assert_eq!(subject.number, 8404);
    }

    #[test]
    fn converts_graphql_review_dismissal_to_timeline_event() {
        let item = serde_json::from_value(serde_json::json!({
            "__typename": "ReviewDismissedEvent",
            "actor": { "__typename": "User", "login": "chaosvolt" },
            "dismissalMessage": "one more test soonmish",
            "createdAt": "2026-06-09T06:46:03Z",
        }))
        .expect("review dismissed event");
        let event = graphql_timeline_event(&item);

        assert_eq!(event.event.as_deref(), Some("review_dismissed"));
        assert_eq!(
            event.actor.as_ref().map(|user| user.login.as_str()),
            Some("chaosvolt")
        );
        assert_eq!(
            event
                .dismissed_review
                .as_ref()
                .and_then(|review| review.dismissal_message.as_deref()),
            Some("one more test soonmish")
        );
    }

    #[test]
    fn converts_graphql_review_comments_to_review_body_fallback() {
        let item = serde_json::from_value(serde_json::json!({
            "__typename": "PullRequestReview",
            "state": "COMMENTED",
            "author": { "__typename": "User", "login": "reviewer" },
            "body": "   ",
            "comments": {
                "nodes": [
                    {
                        "author": { "__typename": "User", "login": "reviewer" },
                        "body": "older inline comment",
                        "createdAt": "2026-03-30T04:08:52Z"
                    },
                    {
                        "author": { "__typename": "User", "login": "reviewer" },
                        "body": "newer inline comment",
                        "createdAt": "2026-03-30T04:09:52Z"
                    }
                ]
            },
            "createdAt": "2026-03-30T04:10:52Z",
        }))
        .expect("review event");
        let event = graphql_timeline_event(&item);

        assert_eq!(event.event.as_deref(), Some("reviewed"));
        assert_eq!(
            event.actor.as_ref().map(|user| user.login.as_str()),
            Some("reviewer")
        );
        assert_eq!(event.body.as_deref(), Some("newer inline comment"));
    }

    #[test]
    fn converts_graphql_bot_commit_to_timeline_event() {
        let commit = serde_json::from_value(serde_json::json!({
            "messageHeadline": "style(autofix.ci): automated formatting",
            "authoredDate": "2026-03-30T04:08:52Z",
            "author": {
                "name": "autofix-ci[bot]",
                "user": { "login": "autofix-ci[bot]" }
            },
            "committer": null,
        }))
        .expect("commit");
        let event = graphql_commit_event(&commit);

        assert_eq!(event.event.as_deref(), Some("committed"));
        assert_eq!(
            event.author.as_ref().map(|user| user.login.as_str()),
            Some("autofix-ci[bot]")
        );
        assert_eq!(
            event.author.as_ref().and_then(|user| user.kind.as_deref()),
            Some("Bot")
        );
        assert_eq!(
            event.message.as_deref(),
            Some("style(autofix.ci): automated formatting")
        );
    }
}

use anyhow::{Context, Result};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED,
    USER_AGENT,
};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use serde_json::json;

use crate::config::GitHubConfig;

use super::auth::resolve_token;
use super::model::{PollResult, PullRequestDetails, Thread, TimelineEvent};
use super::timeline::timeline_url;

const API_VERSION_HEADER: HeaderName = HeaderName::from_static("x-github-api-version");
const POLL_INTERVAL_HEADER: HeaderName = HeaderName::from_static("x-poll-interval");
const PULL_REQUEST_ENRICHMENT_QUERY: &str = r#"
query PullRequestEnrichment($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      merged
      mergedBy {
        __typename
        ... on User { login }
        ... on Bot { login }
      }
      timelineItems(
        last: 20
        itemTypes: [
          PULL_REQUEST_REVIEW
          ISSUE_COMMENT
          REVIEW_REQUESTED_EVENT
          REVIEW_REQUEST_REMOVED_EVENT
          MERGED_EVENT
          CLOSED_EVENT
          REOPENED_EVENT
          READY_FOR_REVIEW_EVENT
          CONVERT_TO_DRAFT_EVENT
          LABELED_EVENT
          UNLABELED_EVENT
          ASSIGNED_EVENT
          UNASSIGNED_EVENT
        ]
      ) {
        nodes {
          __typename
          ... on PullRequestReview {
            state
            author {
              __typename
              login
            }
            body
            createdAt
          }
          ... on IssueComment {
            author {
              __typename
              login
            }
            body
            createdAt
          }
          ... on ReviewRequestedEvent {
            actor {
              __typename
              login
            }
            requestedReviewer {
              __typename
              ... on User { login }
              ... on Bot { login }
              ... on Team { slug }
            }
            createdAt
          }
          ... on ReviewRequestRemovedEvent {
            actor {
              __typename
              login
            }
            requestedReviewer {
              __typename
              ... on User { login }
              ... on Bot { login }
              ... on Team { slug }
            }
            createdAt
          }
          ... on MergedEvent {
            actor {
              __typename
              login
            }
            createdAt
          }
          ... on ClosedEvent {
            actor {
              __typename
              login
            }
            createdAt
          }
          ... on ReopenedEvent {
            actor {
              __typename
              login
            }
            createdAt
          }
          ... on ReadyForReviewEvent {
            actor {
              __typename
              login
            }
            createdAt
          }
          ... on ConvertToDraftEvent {
            actor {
              __typename
              login
            }
            createdAt
          }
          ... on LabeledEvent {
            actor {
              __typename
              login
            }
            label { name }
            createdAt
          }
          ... on UnlabeledEvent {
            actor {
              __typename
              login
            }
            label { name }
            createdAt
          }
          ... on AssignedEvent {
            actor {
              __typename
              login
            }
            assignee {
              __typename
              ... on User { login }
              ... on Bot { login }
            }
            createdAt
          }
          ... on UnassignedEvent {
            actor {
              __typename
              login
            }
            assignee {
              __typename
              ... on User { login }
              ... on Bot { login }
            }
            createdAt
          }
        }
      }
      commits(last: 10) {
        nodes {
          commit {
            messageHeadline
            authoredDate
            author {
              name
              user { login }
            }
            committer {
              name
              user { login }
            }
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlPullRequestPayload {
    repository: Option<GraphQlRepository>,
}

#[derive(Debug, Deserialize)]
struct GraphQlRepository {
    #[serde(rename = "pullRequest")]
    pull_request: Option<GraphQlPullRequest>,
}

#[derive(Debug, Deserialize)]
struct GraphQlPullRequest {
    merged: bool,
    #[serde(rename = "mergedBy")]
    merged_by: Option<GraphQlActor>,
    commits: GraphQlCommitConnection,
    #[serde(rename = "timelineItems")]
    timeline_items: GraphQlTimelineConnection,
}

#[derive(Debug, Deserialize)]
struct GraphQlTimelineConnection {
    nodes: Vec<GraphQlTimelineItem>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum GraphQlTimelineItem {
    PullRequestReview {
        state: Option<String>,
        author: Option<GraphQlActor>,
        body: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    IssueComment {
        author: Option<GraphQlActor>,
        body: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    ReviewRequestedEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "requestedReviewer")]
        requested_reviewer: Option<GraphQlRequestedReviewer>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    ReviewRequestRemovedEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "requestedReviewer")]
        requested_reviewer: Option<GraphQlRequestedReviewer>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    MergedEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    ClosedEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    ReopenedEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    ReadyForReviewEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    ConvertToDraftEvent {
        actor: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    LabeledEvent {
        actor: Option<GraphQlActor>,
        label: Option<GraphQlLabel>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    UnlabeledEvent {
        actor: Option<GraphQlActor>,
        label: Option<GraphQlLabel>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    AssignedEvent {
        actor: Option<GraphQlActor>,
        assignee: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
    UnassignedEvent {
        actor: Option<GraphQlActor>,
        assignee: Option<GraphQlActor>,
        #[serde(rename = "createdAt")]
        created_at: String,
    },
}

#[derive(Debug, Deserialize)]
struct GraphQlLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlCommitConnection {
    nodes: Vec<GraphQlCommitNode>,
}

#[derive(Debug, Deserialize)]
struct GraphQlCommitNode {
    commit: GraphQlCommit,
}

#[derive(Debug, Deserialize)]
struct GraphQlCommit {
    #[serde(rename = "messageHeadline")]
    message_headline: String,
    #[serde(rename = "authoredDate")]
    authored_date: String,
    author: Option<GraphQlCommitSignature>,
    committer: Option<GraphQlCommitSignature>,
}

#[derive(Debug, Deserialize)]
struct GraphQlCommitSignature {
    name: Option<String>,
    user: Option<GraphQlCommitUser>,
}

#[derive(Debug, Deserialize)]
struct GraphQlCommitUser {
    login: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GraphQlActor {
    #[serde(rename = "__typename")]
    kind: String,
    login: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum GraphQlRequestedReviewer {
    User { login: String },
    Bot { login: String },
    Team { slug: String },
}

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

    pub async fn pull_request_enrichment(
        &self,
        subject_url: &str,
    ) -> Result<(PullRequestDetails, Vec<TimelineEvent>)> {
        let subject = parse_pull_request_subject(subject_url)?;
        let response = self
            .client
            .post(self.endpoint("/graphql")?)
            .headers(self.headers()?)
            .json(&json!({
                "query": PULL_REQUEST_ENRICHMENT_QUERY,
                "variables": {
                    "owner": subject.owner,
                    "name": subject.repo,
                    "number": subject.number,
                }
            }))
            .send()
            .await
            .context("failed to fetch pull request enrichment")?
            .error_for_status()
            .context("pull request enrichment request failed")?
            .json::<GraphQlResponse<GraphQlPullRequestPayload>>()
            .await
            .context("failed to decode pull request enrichment")?;

        if !response.errors.is_empty() {
            anyhow::bail!(
                "pull request enrichment query failed: {}",
                response
                    .errors
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
                merged_by: pull_request.merged_by.clone().map(graphql_actor_to_user),
            },
            graphql_timeline_to_rest(&pull_request),
        ))
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
        .map(graphql_timeline_event)
        .collect::<Vec<_>>();

    timeline.extend(
        pull_request
            .commits
            .nodes
            .iter()
            .map(|node| graphql_commit_event(&node.commit)),
    );

    timeline
}

fn graphql_timeline_event(item: &GraphQlTimelineItem) -> TimelineEvent {
    match item {
        GraphQlTimelineItem::PullRequestReview {
            state,
            author,
            body,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("reviewed")),
            actor: author.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: body.clone(),
            message: None,
            commit: None,
            state: state.clone(),
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: Some(created_at.clone()),
        },
        GraphQlTimelineItem::IssueComment {
            author,
            body,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("commented")),
            actor: author.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: body.clone(),
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
        GraphQlTimelineItem::ReviewRequestedEvent {
            actor,
            requested_reviewer,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("review_requested")),
            actor: actor.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: actor.clone().map(graphql_actor_to_user),
            requested_reviewer: requested_reviewer
                .as_ref()
                .and_then(graphql_requested_reviewer_user),
            requested_team: requested_reviewer
                .as_ref()
                .and_then(graphql_requested_reviewer_team),
            label: None,
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
        GraphQlTimelineItem::ReviewRequestRemovedEvent {
            actor,
            requested_reviewer,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("review_request_removed")),
            actor: actor.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: actor.clone().map(graphql_actor_to_user),
            requested_reviewer: requested_reviewer
                .as_ref()
                .and_then(graphql_requested_reviewer_user),
            requested_team: requested_reviewer
                .as_ref()
                .and_then(graphql_requested_reviewer_team),
            label: None,
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
        GraphQlTimelineItem::MergedEvent { actor, created_at } => simple_timeline_event(
            "merged",
            actor.clone().map(graphql_actor_to_user),
            Some(created_at.clone()),
        ),
        GraphQlTimelineItem::ClosedEvent { actor, created_at } => simple_timeline_event(
            "closed",
            actor.clone().map(graphql_actor_to_user),
            Some(created_at.clone()),
        ),
        GraphQlTimelineItem::ReopenedEvent { actor, created_at } => simple_timeline_event(
            "reopened",
            actor.clone().map(graphql_actor_to_user),
            Some(created_at.clone()),
        ),
        GraphQlTimelineItem::ReadyForReviewEvent { actor, created_at } => simple_timeline_event(
            "ready_for_review",
            actor.clone().map(graphql_actor_to_user),
            Some(created_at.clone()),
        ),
        GraphQlTimelineItem::ConvertToDraftEvent { actor, created_at } => simple_timeline_event(
            "convert_to_draft",
            actor.clone().map(graphql_actor_to_user),
            Some(created_at.clone()),
        ),
        GraphQlTimelineItem::LabeledEvent {
            actor,
            label,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("labeled")),
            actor: actor.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: label.as_ref().map(|label| super::model::Label {
                name: label.name.clone(),
            }),
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
        GraphQlTimelineItem::UnlabeledEvent {
            actor,
            label,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("unlabeled")),
            actor: actor.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: label.as_ref().map(|label| super::model::Label {
                name: label.name.clone(),
            }),
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
        GraphQlTimelineItem::AssignedEvent {
            actor,
            assignee,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("assigned")),
            actor: actor.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: assignee.clone().map(graphql_actor_to_user),
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
        GraphQlTimelineItem::UnassignedEvent {
            actor,
            assignee,
            created_at,
        } => TimelineEvent {
            event: Some(String::from("unassigned")),
            actor: actor.clone().map(graphql_actor_to_user),
            user: None,
            author: None,
            committer: None,
            assignee: assignee.clone().map(graphql_actor_to_user),
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(created_at.clone()),
            updated_at: None,
            submitted_at: None,
        },
    }
}

fn graphql_commit_event(commit: &GraphQlCommit) -> TimelineEvent {
    let actor = commit
        .author
        .as_ref()
        .and_then(commit_signature_to_user)
        .or_else(|| commit.committer.as_ref().and_then(commit_signature_to_user));

    TimelineEvent {
        event: Some(String::from("committed")),
        actor: None,
        user: None,
        author: actor,
        committer: None,
        assignee: None,
        review_requester: None,
        requested_reviewer: None,
        requested_team: None,
        label: None,
        dismissed_review: None,
        body: None,
        message: Some(commit.message_headline.clone()),
        commit: None,
        state: None,
        created_at: Some(commit.authored_date.clone()),
        updated_at: None,
        submitted_at: None,
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
        user: None,
        author: None,
        committer: None,
        assignee: None,
        review_requester: None,
        requested_reviewer: None,
        requested_team: None,
        label: None,
        dismissed_review: None,
        body: None,
        message: None,
        commit: None,
        state: None,
        created_at,
        updated_at: None,
        submitted_at: None,
    }
}

fn graphql_actor_to_user(actor: GraphQlActor) -> super::model::User {
    super::model::User {
        login: actor.login,
        kind: Some(actor.kind),
    }
}

fn graphql_requested_reviewer_user(
    reviewer: &GraphQlRequestedReviewer,
) -> Option<super::model::User> {
    match reviewer {
        GraphQlRequestedReviewer::User { login } | GraphQlRequestedReviewer::Bot { login } => {
            Some(super::model::User {
                login: login.clone(),
                kind: login.ends_with("[bot]").then(|| String::from("Bot")),
            })
        }
        GraphQlRequestedReviewer::Team { .. } => None,
    }
}

fn graphql_requested_reviewer_team(
    reviewer: &GraphQlRequestedReviewer,
) -> Option<super::model::Team> {
    match reviewer {
        GraphQlRequestedReviewer::Team { slug } => Some(super::model::Team { slug: slug.clone() }),
        GraphQlRequestedReviewer::User { .. } | GraphQlRequestedReviewer::Bot { .. } => None,
    }
}

fn commit_signature_to_user(signature: &GraphQlCommitSignature) -> Option<super::model::User> {
    let login = signature
        .user
        .as_ref()
        .map(|user| user.login.clone())
        .or_else(|| signature.name.clone())?;

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
    fn converts_graphql_bot_commit_to_timeline_event() {
        let event = graphql_commit_event(&GraphQlCommit {
            message_headline: String::from("style(autofix.ci): automated formatting"),
            authored_date: String::from("2026-03-30T04:08:52Z"),
            author: Some(GraphQlCommitSignature {
                name: Some(String::from("autofix-ci[bot]")),
                user: Some(GraphQlCommitUser {
                    login: String::from("autofix-ci[bot]"),
                }),
            }),
            committer: None,
        });

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

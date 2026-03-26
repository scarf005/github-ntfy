use serde::Deserialize;

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

pub struct PollResult {
    pub notifications: Vec<Thread>,
    pub last_modified: Option<String>,
    pub poll_interval_secs: Option<u64>,
}

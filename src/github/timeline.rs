use anyhow::Result;

use super::model::{TimelineActivity, TimelineEvent, User};

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

pub fn timeline_url(subject_url: &str) -> Result<String> {
    if subject_url.contains("/pulls/") {
        return Ok(subject_url.replace("/pulls/", "/issues/") + "/timeline?per_page=100");
    }

    if subject_url.contains("/issues/") {
        return Ok(format!("{subject_url}/timeline?per_page=100"));
    }

    anyhow::bail!("unsupported subject URL for timeline: {subject_url}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{Team, User};

    fn user(login: &str, kind: Option<&str>) -> User {
        User {
            login: String::from(login),
            kind: kind.map(String::from),
        }
    }

    fn committed_event(actor: &str, message: &str, created_at: &str) -> TimelineEvent {
        TimelineEvent {
            event: Some(String::from("committed")),
            actor: None,
            user: None,
            author: Some(user(actor, None)),
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: None,
            message: Some(String::from(message)),
            commit: None,
            state: None,
            created_at: Some(String::from(created_at)),
            updated_at: None,
            submitted_at: None,
        }
    }

    #[test]
    fn groups_recent_commits_for_same_actor() {
        let timeline = vec![
            committed_event("alice", "feat: one\n\nbody", "2026-03-24T00:00:00Z"),
            committed_event("alice", "fix: two", "2026-03-24T01:00:00Z"),
            committed_event("bob", "chore: three", "2026-03-24T02:00:00Z"),
        ];

        let activity = TimelineActivity::from_timeline(&timeline).expect("activity");

        assert_eq!(activity.kind, "committed");
        assert_eq!(activity.actor, "bob");
        assert_eq!(activity.commit_count, Some(1));
        assert_eq!(activity.detail.as_deref(), Some("chore: three"));
    }

    #[test]
    fn keeps_requested_team_in_review_request_detail() {
        let timeline = vec![TimelineEvent {
            event: Some(String::from("review_requested")),
            actor: Some(user("actor", None)),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: Some(user("carol", None)),
            requested_reviewer: None,
            requested_team: Some(Team {
                slug: String::from("backend"),
            }),
            label: None,
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(String::from("2026-03-24T00:00:00Z")),
            updated_at: None,
            submitted_at: None,
        }];

        let activity = TimelineActivity::from_timeline(&timeline).expect("activity");

        assert_eq!(activity.kind, "review_requested");
        assert_eq!(activity.actor, "carol");
        assert_eq!(activity.detail.as_deref(), Some("Requested from @backend"));
    }

    #[test]
    fn detects_bot_users_from_type_or_suffix() {
        assert!(user("renovate[bot]", None).is_bot());
        assert!(user("app", Some("Bot")).is_bot());
        assert!(!user("alice", Some("User")).is_bot());
    }

    #[test]
    fn converts_pull_urls_to_timeline_urls() {
        let url = timeline_url("https://api.github.com/repos/octo/repo/pulls/42").expect("url");
        assert_eq!(
            url,
            "https://api.github.com/repos/octo/repo/issues/42/timeline?per_page=100"
        );
    }
}

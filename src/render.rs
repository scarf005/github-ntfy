use anyhow::Result;

use crate::github::{PullRequestDetails, Thread, TimelineActivity, TimelineEvent};

const DEFAULT_ICON_URL: &str =
    "https://github.githubassets.com/images/modules/logos_page/GitHub-Mark.png";

#[derive(Debug, Clone)]
pub struct RenderedNotification {
    pub dedupe_key: String,
    pub sequence_id: String,
    pub title: String,
    pub message: String,
    pub actions: Option<String>,
    pub click_url: String,
    pub icon_url: String,
    pub tags: String,
    pub priority: u8,
}

pub fn render_notification(
    thread: &Thread,
    pull_request: Option<&PullRequestDetails>,
    timeline: Option<&[TimelineEvent]>,
) -> Result<RenderedNotification> {
    let base_title = thread
        .subject
        .title
        .clone()
        .unwrap_or_else(|| thread.repository.full_name.clone());
    let base_message = base_message(thread);
    let click_url = click_url(thread);
    let icon_url = thread
        .repository
        .owner
        .as_ref()
        .and_then(|owner| owner.avatar_url.clone())
        .unwrap_or_else(|| String::from(DEFAULT_ICON_URL));
    let tags = build_tags(thread);
    let priority = priority(thread);
    let reason = thread.reason.as_deref().unwrap_or("notification");

    let (title, message) = match thread.subject.kind.as_deref() {
        Some("PullRequest") => {
            enrich_pull_request(&base_title, &base_message, reason, pull_request, timeline)
        }
        Some("Issue") => enrich_issue(&base_title, &base_message, reason, timeline),
        _ => (base_title, base_message),
    };

    Ok(RenderedNotification {
        dedupe_key: format!("{}|{}", thread.id, thread.updated_at),
        sequence_id: format!("github-thread-{}", thread.id),
        title,
        message,
        actions: None,
        click_url,
        icon_url,
        tags,
        priority,
    })
}

fn enrich_pull_request(
    base_title: &str,
    base_message: &str,
    reason: &str,
    pull_request: Option<&PullRequestDetails>,
    timeline: Option<&[TimelineEvent]>,
) -> (String, String) {
    let Some(timeline) = timeline else {
        return (String::from(base_title), String::from(base_message));
    };
    let Some(activity) = TimelineActivity::from_timeline(timeline) else {
        return (String::from(base_title), String::from(base_message));
    };

    let merged_by =
        pull_request.and_then(|pull| pull.merged_by.as_ref().map(|user| user.login.clone()));
    let is_merged = pull_request.is_some_and(|pull| pull.merged);
    let actor = merged_by.unwrap_or_else(|| activity.actor.clone());

    let summary = match activity.kind.as_str() {
        "review_approved" => format!("@{} approved {}", actor, base_title),
        "review_changes_requested" => format!("@{} requested changes on {}", actor, base_title),
        "reviewed" => format!("@{} reviewed {}", actor, base_title),
        "commented" if reason == "mention" || reason == "team_mention" => {
            format!("@{} mentioned you in {}", actor, base_title)
        }
        "commented" => format!("@{} commented on {}", actor, base_title),
        "committed" => format!(
            "@{} pushed {} {} to {}",
            actor,
            activity.commit_count.unwrap_or(1),
            pluralize_commit(activity.commit_count.unwrap_or(1)),
            base_title
        ),
        "merged" => format!("@{} merged {}", actor, base_title),
        "review_requested" => format!("@{} requested review on {}", actor, base_title),
        "review_request_removed" => format!("@{} removed review request on {}", actor, base_title),
        "review_dismissed" => format!("@{} dismissed review on {}", actor, base_title),
        "closed" if is_merged => format!("@{} merged {}", actor, base_title),
        "closed" => format!("@{} closed {}", actor, base_title),
        "reopened" => format!("@{} reopened {}", actor, base_title),
        "ready_for_review" => format!("@{} marked {} ready for review", actor, base_title),
        "convert_to_draft" => format!("@{} converted {} to draft", actor, base_title),
        _ => String::from(base_title),
    };

    let message = prepend_summary(
        &summary,
        &detail_message(base_message, activity.detail.as_deref()),
    );

    (String::from(base_title), message)
}

fn enrich_issue(
    base_title: &str,
    base_message: &str,
    reason: &str,
    timeline: Option<&[TimelineEvent]>,
) -> (String, String) {
    let Some(timeline) = timeline else {
        return (String::from(base_title), String::from(base_message));
    };
    let Some(activity) = TimelineActivity::from_timeline(timeline) else {
        return (String::from(base_title), String::from(base_message));
    };

    let summary = match activity.kind.as_str() {
        "commented" if reason == "mention" || reason == "team_mention" => {
            format!("@{} mentioned you in {}", activity.actor, base_title)
        }
        "commented" => format!("@{} commented on {}", activity.actor, base_title),
        "closed" => format!("@{} closed {}", activity.actor, base_title),
        "reopened" => format!("@{} reopened {}", activity.actor, base_title),
        "assigned" => format!("@{} assigned {}", activity.actor, base_title),
        "unassigned" => format!("@{} unassigned {}", activity.actor, base_title),
        "labeled" => format!("@{} labeled {}", activity.actor, base_title),
        "unlabeled" => format!("@{} unlabeled {}", activity.actor, base_title),
        _ => String::from(base_title),
    };

    let message = prepend_summary(
        &summary,
        &detail_message(base_message, activity.detail.as_deref()),
    );

    (String::from(base_title), message)
}

fn prepend_summary(summary: &str, message: &str) -> String {
    if summary.trim().is_empty() {
        return String::from(message);
    }

    format!("{}\n{}", summary, message)
}

fn detail_message(base_message: &str, detail: Option<&str>) -> String {
    if let Some(detail) = detail.filter(|detail| !detail.trim().is_empty()) {
        format!("{}\n{}", trim_multiline_text(detail), base_message)
    } else {
        String::from(base_message)
    }
}

fn base_message(thread: &Thread) -> String {
    let repo = &thread.repository.full_name;
    let subject = subject_label(thread.subject.kind.as_deref());

    match thread.reason.as_deref() {
        Some("assign") => format!("{subject} assigned to you in {repo}"),
        Some("author") => format!(
            "Activity on your {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("comment") => format!(
            "New comment on {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("ci_activity") => format!(
            "CI activity on {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("invitation") => format!(
            "Invitation for {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("manual") => format!(
            "Manual notification for {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("mention") => format!(
            "Mentioned you in {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("review_requested") => format!(
            "Review requested on {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some("security_alert") => format!("Security alert in {repo}"),
        Some("state_change") => format!("{subject} state changed in {repo}"),
        Some("subscribed") => format!("{subject} update in {repo}"),
        Some("team_mention") => format!(
            "Mentioned your team in {subject_lower} in {repo}",
            subject_lower = subject.to_ascii_lowercase()
        ),
        Some(reason) => format!("{subject} notification ({reason}) in {repo}"),
        None => format!("{subject} notification in {repo}"),
    }
}

fn subject_label(kind: Option<&str>) -> &'static str {
    match kind {
        Some("PullRequest") => "Pull request",
        Some("Issue") => "Issue",
        Some("Commit") => "Commit",
        Some("Discussion") => "Discussion",
        Some("Release") => "Release",
        _ => "Notification",
    }
}

fn trim_multiline_text(input: &str) -> String {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn pluralize_commit(count: usize) -> &'static str {
    if count == 1 { "commit" } else { "commits" }
}

fn click_url(thread: &Thread) -> String {
    let Some(subject_url) = thread.subject.url.as_deref() else {
        return thread.repository.html_url.clone();
    };

    let base = subject_url.replace("https://api.github.com/repos/", "https://github.com/");
    match thread.subject.kind.as_deref() {
        Some("PullRequest") => base
            .replace("/pulls/", "/pull/")
            .replace("/issues/", "/pull/"),
        Some("Commit") => base.replace("/commits/", "/commit/"),
        Some("Release") => {
            if let Some(stripped) = base.strip_suffix('/') {
                stripped.to_string()
            } else {
                base
            }
        }
        _ => base,
    }
}

fn build_tags(thread: &Thread) -> String {
    let mut tags = vec![String::from("github")];
    let type_tag = match thread.subject.kind.as_deref() {
        Some("PullRequest") => "pr",
        Some("Issue") => "issue",
        Some("Commit") => "commit",
        Some("Discussion") => "discussion",
        Some("Release") => "release",
        _ => "notification",
    };
    tags.push(String::from(type_tag));
    if let Some(reason) = &thread.reason {
        tags.push(reason.clone());
    }
    tags.sort();
    tags.dedup();
    tags.join(",")
}

fn priority(thread: &Thread) -> u8 {
    match thread.reason.as_deref() {
        Some("security_alert") => 5,
        Some("mention" | "team_mention" | "review_requested") => 4,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{Label, Owner, Repository, Subject, Thread, TimelineEvent, User};

    fn sample_thread() -> Thread {
        Thread {
            id: String::from("1"),
            unread: true,
            updated_at: String::from("2026-03-25T00:00:00Z"),
            reason: Some(String::from("subscribed")),
            repository: Repository {
                full_name: String::from("octo/repo"),
                html_url: String::from("https://github.com/octo/repo"),
                owner: Some(Owner {
                    avatar_url: Some(String::from(
                        "https://avatars.githubusercontent.com/u/1?v=4",
                    )),
                }),
            },
            subject: Subject {
                title: Some(String::from("Fix pull link")),
                kind: Some(String::from("PullRequest")),
                url: Some(String::from(
                    "https://api.github.com/repos/octo/repo/pulls/42",
                )),
            },
        }
    }

    fn sample_issue_thread() -> Thread {
        Thread {
            id: String::from("2"),
            unread: true,
            updated_at: String::from("2026-03-25T00:00:00Z"),
            reason: Some(String::from("subscribed")),
            repository: Repository {
                full_name: String::from("octo/repo"),
                html_url: String::from("https://github.com/octo/repo"),
                owner: Some(Owner {
                    avatar_url: Some(String::from(
                        "https://avatars.githubusercontent.com/u/1?v=4",
                    )),
                }),
            },
            subject: Subject {
                title: Some(String::from("Issue title")),
                kind: Some(String::from("Issue")),
                url: Some(String::from(
                    "https://api.github.com/repos/octo/repo/issues/7",
                )),
            },
        }
    }

    #[test]
    fn renders_pull_url_from_api_url() {
        let rendered = render_notification(&sample_thread(), None, None).expect("rendered");
        assert_eq!(rendered.click_url, "https://github.com/octo/repo/pull/42");
        assert_eq!(rendered.sequence_id, "github-thread-1");
        assert_eq!(rendered.message, "Pull request update in octo/repo");
    }

    #[test]
    fn renders_commit_push_title_from_timeline() {
        let thread = sample_thread();
        let timeline = vec![TimelineEvent {
            event: Some(String::from("committed")),
            actor: None,
            user: None,
            author: Some(User {
                login: String::from("foo"),
                kind: None,
            }),
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: None,
            message: Some(String::from("feat: improve notifier\n\nbody")),
            commit: None,
            state: None,
            created_at: Some(String::from("2026-03-25T00:00:00Z")),
            updated_at: None,
            submitted_at: None,
        }];

        let rendered = render_notification(&thread, None, Some(&timeline)).expect("rendered");
        assert_eq!(rendered.title, "Fix pull link");
        assert!(
            rendered
                .message
                .starts_with("@foo pushed 1 commit to Fix pull link\n")
        );
        assert!(rendered.message.contains("feat: improve notifier"));
    }

    #[test]
    fn renders_issue_mention_title_from_reason() {
        let mut thread = sample_issue_thread();
        thread.reason = Some(String::from("mention"));
        let timeline = vec![TimelineEvent {
            event: Some(String::from("commented")),
            actor: Some(User {
                login: String::from("bar"),
                kind: None,
            }),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: None,
            dismissed_review: None,
            body: Some(String::from("ping @you")),
            message: None,
            commit: None,
            state: None,
            created_at: Some(String::from("2026-03-25T00:00:00Z")),
            updated_at: None,
            submitted_at: None,
        }];

        let rendered = render_notification(&thread, None, Some(&timeline)).expect("rendered");
        assert_eq!(rendered.title, "Issue title");
        assert!(
            rendered
                .message
                .starts_with("@bar mentioned you in Issue title\n")
        );
        assert!(rendered.message.contains("ping @you"));
        assert!(
            rendered
                .message
                .contains("Mentioned you in issue in octo/repo")
        );
    }

    #[test]
    fn renders_issue_closed_title_from_timeline() {
        let thread = sample_issue_thread();
        let timeline = vec![TimelineEvent {
            event: Some(String::from("closed")),
            actor: Some(User {
                login: String::from("bar"),
                kind: None,
            }),
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
            created_at: Some(String::from("2026-03-25T00:00:00Z")),
            updated_at: None,
            submitted_at: None,
        }];

        let rendered = render_notification(&thread, None, Some(&timeline)).expect("rendered");
        assert_eq!(rendered.title, "Issue title");
        assert!(rendered.message.starts_with("@bar closed Issue title\n"));
    }

    #[test]
    fn renders_issue_label_detail_from_timeline() {
        let thread = sample_issue_thread();
        let timeline = vec![TimelineEvent {
            event: Some(String::from("labeled")),
            actor: Some(User {
                login: String::from("bar"),
                kind: None,
            }),
            user: None,
            author: None,
            committer: None,
            assignee: None,
            review_requester: None,
            requested_reviewer: None,
            requested_team: None,
            label: Some(Label {
                name: String::from("bug"),
            }),
            dismissed_review: None,
            body: None,
            message: None,
            commit: None,
            state: None,
            created_at: Some(String::from("2026-03-25T00:00:00Z")),
            updated_at: None,
            submitted_at: None,
        }];

        let rendered = render_notification(&thread, None, Some(&timeline)).expect("rendered");
        assert_eq!(rendered.title, "Issue title");
        assert!(rendered.message.starts_with("@bar labeled Issue title\n"));
        assert!(rendered.message.contains("Added label: bug"));
    }
}

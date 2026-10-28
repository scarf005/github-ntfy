use crate::config::BlockRule;
use crate::github::{Thread, TimelineActivity, TimelineEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationIntentKind {
    Activity,
    InitialSubject,
}

#[derive(Debug, Clone)]
pub struct NotificationIntent {
    kind: NotificationIntentKind,
    timeline: Vec<TimelineEvent>,
}

impl NotificationIntent {
    pub fn classify(
        thread: &Thread,
        timeline: Option<Vec<TimelineEvent>>,
        has_existing_notification: bool,
    ) -> Self {
        let timeline = anchor_timeline(thread, timeline.unwrap_or_default());
        let activity = TimelineActivity::from_timeline(&timeline);
        let kind = if is_issue_or_pull_request(thread)
            && !has_existing_notification
            && should_render_initial_subject(thread, activity.as_ref())
        {
            NotificationIntentKind::InitialSubject
        } else {
            NotificationIntentKind::Activity
        };

        Self { kind, timeline }
    }

    pub fn initial_subject() -> Self {
        Self {
            kind: NotificationIntentKind::InitialSubject,
            timeline: Vec::new(),
        }
    }

    pub fn kind(&self) -> NotificationIntentKind {
        self.kind
    }

    pub fn timeline(&self) -> Option<&[TimelineEvent]> {
        match self.kind {
            NotificationIntentKind::Activity => Some(&self.timeline),
            NotificationIntentKind::InitialSubject => None,
        }
    }

    pub fn should_fallback_to_initial_subject(
        &self,
        rule: &BlockRule,
        thread: &Thread,
        has_existing_notification: bool,
    ) -> bool {
        self.kind == NotificationIntentKind::Activity
            && rule.activity.is_some()
            && is_issue_or_pull_request(thread)
            && !has_existing_notification
    }
}

fn is_issue_or_pull_request(thread: &Thread) -> bool {
    matches!(
        thread.subject.kind.as_deref(),
        Some("PullRequest" | "Issue")
    )
}

fn should_render_initial_subject(thread: &Thread, activity: Option<&TimelineActivity>) -> bool {
    match activity {
        None => true,
        Some(activity) => {
            latest_comment_points_to_subject(thread) && activity.kind.as_str() == "committed"
        }
    }
}

fn anchor_timeline(thread: &Thread, timeline: Vec<TimelineEvent>) -> Vec<TimelineEvent> {
    let Some(latest_comment_url) = thread.subject.latest_comment_url.as_deref() else {
        return timeline;
    };

    if latest_comment_points_to_subject(thread) || !reason_can_anchor_latest_comment(thread) {
        return timeline;
    }

    timeline
        .iter()
        .find(|event| {
            event
                .url
                .as_deref()
                .is_some_and(|url| latest_comment_url_matches(thread, latest_comment_url, url))
                && event_time(event) == thread.updated_at
        })
        .cloned()
        .map(|event| vec![event])
        .unwrap_or(timeline)
}

fn reason_can_anchor_latest_comment(thread: &Thread) -> bool {
    matches!(
        thread.reason.as_deref(),
        Some("comment" | "mention" | "team_mention")
    )
}

fn event_time(event: &TimelineEvent) -> &str {
    event
        .submitted_at
        .as_deref()
        .or(event.updated_at.as_deref())
        .or(event.created_at.as_deref())
        .unwrap_or_default()
}

fn latest_comment_points_to_subject(thread: &Thread) -> bool {
    thread
        .subject
        .latest_comment_url
        .as_deref()
        .is_some_and(|latest| {
            let Some(subject_url) = thread.subject.url.as_deref() else {
                return false;
            };

            latest == subject_url
                || (subject_url.contains("/pulls/")
                    && latest == subject_url.replace("/pulls/", "/issues/"))
        })
}

fn latest_comment_url_matches(thread: &Thread, latest_comment_url: &str, event_url: &str) -> bool {
    event_url == latest_comment_url
        || latest_comment_html_alias(thread, latest_comment_url).as_deref() == Some(event_url)
}

fn latest_comment_html_alias(thread: &Thread, latest_comment_url: &str) -> Option<String> {
    let id = latest_comment_url.rsplit('/').next()?;
    let subject_url = thread.subject.url.as_deref()?;
    let mut subject_url =
        subject_url.replace("https://api.github.com/repos/", "https://github.com/");
    if thread.subject.kind.as_deref() == Some("PullRequest") {
        subject_url = subject_url
            .replace("/pulls/", "/pull/")
            .replace("/issues/", "/pull/");
    }

    if latest_comment_url.contains("/issues/comments/") {
        return Some(format!("{subject_url}#issuecomment-{id}"));
    }

    latest_comment_url
        .contains("/pulls/comments/")
        .then(|| format!("{subject_url}#discussion_r{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{
        Label, Repository, Subject, TimelineSource, TimelineSourceIssue, TimelineSourcePullRequest,
        User,
    };

    fn thread(kind: &str) -> Thread {
        Thread {
            id: String::from("1"),
            unread: true,
            updated_at: String::from("2026-06-23T01:30:04Z"),
            reason: Some(String::from("subscribed")),
            repository: Repository {
                full_name: String::from("scarf005/Overhaul"),
                html_url: String::from("https://github.com/scarf005/Overhaul"),
                owner: None,
            },
            subject: Subject {
                title: Some(String::from("feat: Add Lang files")),
                kind: Some(String::from(kind)),
                url: Some(subject_url(kind)),
                latest_comment_url: Some(subject_url(kind)),
            },
        }
    }

    fn subject_url(kind: &str) -> String {
        let surface = if kind == "PullRequest" {
            "pulls"
        } else {
            "issues"
        };
        format!("https://api.github.com/repos/scarf005/Overhaul/{surface}/1")
    }

    fn commit_event() -> TimelineEvent {
        TimelineEvent {
            event: Some(String::from("committed")),
            author: Some(User {
                login: String::from("RoyalFox2140"),
                kind: None,
            }),
            message: Some(String::from("Update modinfo.json lethal_zeds")),
            created_at: Some(String::from("2026-06-29T10:46:09Z")),
            ..TimelineEvent::default()
        }
    }

    fn comment_event(url: &str, body: &str, created_at: &str) -> TimelineEvent {
        TimelineEvent {
            event: Some(String::from("commented")),
            url: Some(String::from(url)),
            actor: Some(User {
                login: String::from("chaosvolt"),
                kind: None,
            }),
            body: Some(String::from(body)),
            created_at: Some(String::from(created_at)),
            ..TimelineEvent::default()
        }
    }

    fn labeled_event() -> TimelineEvent {
        TimelineEvent {
            event: Some(String::from("labeled")),
            actor: Some(User {
                login: String::from("github-actions"),
                kind: Some(String::from("Bot")),
            }),
            label: Some(Label {
                name: String::from("JSON"),
            }),
            created_at: Some(String::from("2026-06-23T01:30:04Z")),
            ..TimelineEvent::default()
        }
    }

    #[test]
    fn classifies_thread_without_fetched_timeline_as_activity() {
        let intent = NotificationIntent::classify(&thread("Issue"), None, false);

        assert_eq!(intent.kind(), NotificationIntentKind::Activity);
    }

    #[test]
    fn classifies_first_issue_without_activity_as_initial_subject() {
        let intent = NotificationIntent::classify(&thread("Issue"), Some(Vec::new()), false);

        assert_eq!(intent.kind(), NotificationIntentKind::InitialSubject);
        assert!(intent.timeline().is_none());
    }

    #[test]
    fn classifies_first_subject_with_only_subject_anchor_as_initial_subject() {
        let intent =
            NotificationIntent::classify(&thread("PullRequest"), Some(vec![commit_event()]), false);

        assert_eq!(intent.kind(), NotificationIntentKind::InitialSubject);
        assert!(intent.timeline().is_none());
    }

    #[test]
    fn classifies_closed_via_subject_anchor_as_activity() {
        let mut thread = thread("Issue");
        thread.reason = Some(String::from("subscribed"));
        thread.updated_at = String::from("2026-06-30T06:58:08Z");
        thread.subject.latest_comment_url = thread.subject.url.clone();
        let intent = NotificationIntent::classify(
            &thread,
            Some(vec![
                TimelineEvent {
                    event: Some(String::from("cross-referenced")),
                    actor: Some(User {
                        login: String::from("dedmemdev"),
                        kind: None,
                    }),
                    created_at: Some(String::from("2026-06-30T06:11:18Z")),
                    source: Some(TimelineSource {
                        issue: Some(TimelineSourceIssue {
                            number: 9741,
                            title: String::from("feat(balance): diversify skeleton evolution tree"),
                            html_url: String::from(
                                "https://github.com/cataclysmbn/Cataclysm-BN/pull/9741",
                            ),
                            pull_request: Some(TimelineSourcePullRequest {
                                merged_at: Some(String::from("2026-06-30T06:57:46Z")),
                            }),
                        }),
                    }),
                    ..TimelineEvent::default()
                },
                TimelineEvent {
                    event: Some(String::from("closed")),
                    actor: Some(User {
                        login: String::from("chaosvolt"),
                        kind: None,
                    }),
                    created_at: Some(String::from("2026-06-30T06:57:47Z")),
                    ..TimelineEvent::default()
                },
            ]),
            false,
        );

        assert_eq!(intent.kind(), NotificationIntentKind::Activity);
        let activity = TimelineActivity::from_timeline(intent.timeline().unwrap()).unwrap();
        assert_eq!(activity.kind, "closed_via_pull_request");
    }

    #[test]
    fn keeps_first_subject_with_unknown_anchor_as_activity() {
        let mut thread = thread("PullRequest");
        thread.subject.latest_comment_url = None;
        let intent = NotificationIntent::classify(&thread, Some(vec![commit_event()]), false);

        assert_eq!(intent.kind(), NotificationIntentKind::Activity);
        assert!(intent.timeline().is_some());
    }

    #[test]
    fn keeps_latest_comment_anchor_as_activity_source() {
        let mut thread = thread("PullRequest");
        thread.reason = Some(String::from("comment"));
        thread.updated_at = String::from("2026-06-29T10:46:09Z");
        thread.subject.latest_comment_url = Some(String::from(
            "https://api.github.com/repos/scarf005/Overhaul/issues/comments/2",
        ));
        let intent = NotificationIntent::classify(
            &thread,
            Some(vec![
                comment_event(
                    "https://github.com/scarf005/Overhaul/pull/1#issuecomment-2",
                    "target comment",
                    "2026-06-29T10:46:09Z",
                ),
                comment_event(
                    "https://api.github.com/repos/scarf005/Overhaul/issues/comments/1",
                    "newer unrelated comment",
                    "2026-06-29T10:47:09Z",
                ),
            ]),
            true,
        );

        let timeline = intent.timeline().expect("anchored activity");
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].body.as_deref(), Some("target comment"));
    }

    #[test]
    fn anchors_pull_request_comment_when_subject_uses_issue_url() {
        let mut thread = thread("PullRequest");
        thread.reason = Some(String::from("comment"));
        thread.updated_at = String::from("2026-06-29T10:46:09Z");
        thread.subject.url = Some(String::from(
            "https://api.github.com/repos/scarf005/Overhaul/issues/1",
        ));
        thread.subject.latest_comment_url = Some(String::from(
            "https://api.github.com/repos/scarf005/Overhaul/issues/comments/2",
        ));
        let intent = NotificationIntent::classify(
            &thread,
            Some(vec![
                comment_event(
                    "https://github.com/scarf005/Overhaul/pull/1#issuecomment-2",
                    "target comment",
                    "2026-06-29T10:46:09Z",
                ),
                comment_event(
                    "https://github.com/scarf005/Overhaul/pull/1#issuecomment-1",
                    "newer unrelated comment",
                    "2026-06-29T10:47:09Z",
                ),
            ]),
            true,
        );

        let timeline = intent.timeline().expect("anchored activity");
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].body.as_deref(), Some("target comment"));
    }

    #[test]
    fn ignores_stale_comment_anchor_for_state_change() {
        let mut thread = thread("Issue");
        thread.reason = Some(String::from("state_change"));
        thread.updated_at = String::from("2026-06-29T10:47:09Z");
        thread.subject.latest_comment_url = Some(String::from(
            "https://api.github.com/repos/scarf005/Overhaul/issues/comments/2",
        ));
        let intent = NotificationIntent::classify(
            &thread,
            Some(vec![
                comment_event(
                    "https://api.github.com/repos/scarf005/Overhaul/issues/comments/2",
                    "stale comment",
                    "2026-06-29T10:46:09Z",
                ),
                TimelineEvent {
                    event: Some(String::from("closed")),
                    actor: Some(User {
                        login: String::from("chaosvolt"),
                        kind: None,
                    }),
                    created_at: Some(String::from("2026-06-29T10:47:09Z")),
                    ..TimelineEvent::default()
                },
            ]),
            true,
        );

        let timeline = intent.timeline().expect("state change activity");
        assert_eq!(timeline.len(), 2);
        let activity = TimelineActivity::from_timeline(timeline).expect("activity");
        assert_eq!(activity.kind, "closed");
    }

    #[test]
    fn keeps_existing_issue_without_activity_as_activity_update() {
        let intent = NotificationIntent::classify(&thread("Issue"), Some(Vec::new()), true);

        assert_eq!(intent.kind(), NotificationIntentKind::Activity);
        assert!(intent.timeline().is_some());
    }

    #[test]
    fn falls_back_to_initial_subject_for_filtered_first_activity() {
        let mut thread = thread("PullRequest");
        thread.subject.latest_comment_url = Some(String::from(
            "https://api.github.com/repos/scarf005/Overhaul/issues/comments/1",
        ));
        let intent = NotificationIntent::classify(&thread, Some(vec![labeled_event()]), false);
        let rule = BlockRule {
            activity: Some(String::from("labeled")),
            ..BlockRule::default()
        };

        assert!(TimelineActivity::from_timeline(intent.timeline().unwrap()).is_some());
        assert!(intent.should_fallback_to_initial_subject(&rule, &thread, false));
    }
}

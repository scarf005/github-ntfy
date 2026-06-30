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
        let timeline = timeline.unwrap_or_default();
        let has_activity = TimelineActivity::from_timeline(&timeline).is_some();
        let kind =
            if is_issue_or_pull_request(thread) && !has_existing_notification && !has_activity {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{Label, Repository, Subject, User};

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
                url: Some(String::from(
                    "https://api.github.com/repos/scarf005/Overhaul/issues/1",
                )),
                latest_comment_url: Some(String::from(
                    "https://api.github.com/repos/scarf005/Overhaul/issues/1",
                )),
            },
        }
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
    fn classifies_first_issue_without_activity_as_initial_subject() {
        let intent = NotificationIntent::classify(&thread("Issue"), Some(Vec::new()), false);

        assert_eq!(intent.kind(), NotificationIntentKind::InitialSubject);
        assert!(intent.timeline().is_none());
    }

    #[test]
    fn classifies_first_subject_with_only_subject_anchor_as_initial_subject() {
        let intent = NotificationIntent::classify(&thread("PullRequest"), Some(vec![commit_event()]), false);

        assert_eq!(intent.kind(), NotificationIntentKind::InitialSubject);
        assert!(intent.timeline().is_none());
    }

    #[test]
    fn keeps_existing_issue_without_activity_as_activity_update() {
        let intent = NotificationIntent::classify(&thread("Issue"), Some(Vec::new()), true);

        assert_eq!(intent.kind(), NotificationIntentKind::Activity);
        assert!(intent.timeline().is_some());
    }

    #[test]
    fn falls_back_to_initial_subject_for_filtered_first_activity() {
        let intent = NotificationIntent::classify(
            &thread("PullRequest"),
            Some(vec![labeled_event()]),
            false,
        );
        let rule = BlockRule {
            activity: Some(String::from("labeled")),
            ..BlockRule::default()
        };

        assert!(TimelineActivity::from_timeline(intent.timeline().unwrap()).is_some());
        assert!(intent.should_fallback_to_initial_subject(&rule, &thread("PullRequest"), false));
    }
}

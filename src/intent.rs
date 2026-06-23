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
        _thread: &Thread,
        timeline: Option<Vec<TimelineEvent>>,
        _has_existing_notification: bool,
    ) -> Self {
        Self {
            kind: NotificationIntentKind::Activity,
            timeline: timeline.unwrap_or_default(),
        }
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
        _rule: &BlockRule,
        _thread: &Thread,
        _has_existing_notification: bool,
    ) -> bool {
        false
    }
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
            },
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
    fn keeps_existing_issue_without_activity_as_activity_update() {
        let intent = NotificationIntent::classify(&thread("Issue"), Some(Vec::new()), true);

        assert_eq!(intent.kind(), NotificationIntentKind::Activity);
        assert!(intent.timeline().is_some());
    }

    #[test]
    fn falls_back_to_initial_subject_for_filtered_first_activity() {
        let intent = NotificationIntent::classify(&thread("PullRequest"), Some(vec![labeled_event()]), false);
        let rule = BlockRule {
            activity: Some(String::from("labeled")),
            ..BlockRule::default()
        };

        assert!(TimelineActivity::from_timeline(intent.timeline().unwrap()).is_some());
        assert!(intent.should_fallback_to_initial_subject(&rule, &thread("PullRequest"), false));
    }
}

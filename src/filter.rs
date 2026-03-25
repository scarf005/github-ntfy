use wildmatch::WildMatch;

use crate::config::{BlockRule, FiltersConfig};
use crate::github::{PullRequestDetails, Thread, TimelineActivity, TimelineEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationFacts {
    pub repo_full_name: String,
    pub owner: String,
    pub subject_type: String,
    pub reason: String,
    pub activity: Option<String>,
    pub actor: Option<String>,
    pub actor_is_bot: bool,
}

pub fn build_notification_facts(
    thread: &Thread,
    pull_request: Option<&PullRequestDetails>,
    timeline: Option<&[TimelineEvent]>,
) -> NotificationFacts {
    let activity = TimelineActivity::from_timeline(timeline.unwrap_or(&[]));
    let owner = thread
        .repository
        .full_name
        .split_once('/')
        .map(|(owner, _)| owner)
        .unwrap_or_default()
        .to_string();
    let reason = thread
        .reason
        .clone()
        .unwrap_or_else(|| String::from("notification"));
    let subject_type = thread
        .subject
        .kind
        .clone()
        .unwrap_or_else(|| String::from("Notification"));

    let (actor, actor_is_bot) = match (pull_request, activity.as_ref()) {
        (Some(pull_request), Some(activity))
            if activity.kind == "merged" || (activity.kind == "closed" && pull_request.merged) =>
        {
            let actor = pull_request
                .merged_by
                .as_ref()
                .map(|user| user.login.clone());
            let is_bot = pull_request
                .merged_by
                .as_ref()
                .is_some_and(|user| user.is_bot());
            (
                actor.or_else(|| Some(activity.actor.clone())),
                is_bot || activity.actor_is_bot,
            )
        }
        (_, Some(activity)) => (Some(activity.actor.clone()), activity.actor_is_bot),
        _ => (None, false),
    };

    NotificationFacts {
        repo_full_name: thread.repository.full_name.clone(),
        owner,
        subject_type,
        reason,
        activity: activity.map(|activity| activity.kind),
        actor,
        actor_is_bot,
    }
}

pub fn matching_block_rule<'a>(
    filters: &'a FiltersConfig,
    facts: &NotificationFacts,
) -> Option<&'a BlockRule> {
    filters
        .block
        .iter()
        .filter(|rule| !is_empty_rule(rule))
        .find(|rule| matches_rule(rule, facts))
}

fn is_empty_rule(rule: &BlockRule) -> bool {
    rule.repo.is_none()
        && rule.owner.is_none()
        && rule.actor.is_none()
        && rule.actor_is_bot.is_none()
        && rule.reason.is_none()
        && rule.subject_type.is_none()
        && rule.activity.is_none()
}

fn matches_rule(rule: &BlockRule, facts: &NotificationFacts) -> bool {
    rule.repo
        .as_deref()
        .is_none_or(|pattern| WildMatch::new(pattern).matches(&facts.repo_full_name))
        && rule
            .owner
            .as_deref()
            .is_none_or(|pattern| WildMatch::new(pattern).matches(&facts.owner))
        && rule.actor.as_deref().is_none_or(|pattern| {
            facts
                .actor
                .as_deref()
                .is_some_and(|actor| WildMatch::new(pattern).matches(actor))
        })
        && rule
            .actor_is_bot
            .is_none_or(|expected| facts.actor.is_some() && expected == facts.actor_is_bot)
        && rule
            .reason
            .as_deref()
            .is_none_or(|reason| reason.eq_ignore_ascii_case(&facts.reason))
        && rule
            .subject_type
            .as_deref()
            .is_none_or(|subject_type| subject_type.eq_ignore_ascii_case(&facts.subject_type))
        && rule.activity.as_deref().is_none_or(|activity| {
            facts
                .activity
                .as_deref()
                .is_some_and(|candidate| activity.eq_ignore_ascii_case(candidate))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BlockRule, FiltersConfig};

    fn facts() -> NotificationFacts {
        NotificationFacts {
            repo_full_name: String::from("example-org/api"),
            owner: String::from("example-org"),
            subject_type: String::from("Issue"),
            reason: String::from("mention"),
            activity: Some(String::from("commented")),
            actor: Some(String::from("renovate[bot]")),
            actor_is_bot: true,
        }
    }

    #[test]
    fn matches_block_rule_for_repo_and_bot_actor() {
        let filters = FiltersConfig {
            block: vec![BlockRule {
                name: Some(String::from("ignore example-org bots")),
                repo: Some(String::from("example-org/*")),
                actor_is_bot: Some(true),
                ..BlockRule::default()
            }],
        };

        let matched = matching_block_rule(&filters, &facts()).expect("rule matched");
        assert_eq!(matched.name.as_deref(), Some("ignore example-org bots"));
    }

    #[test]
    fn does_not_match_when_actor_is_human() {
        let filters = FiltersConfig {
            block: vec![BlockRule {
                repo: Some(String::from("example-org/*")),
                actor_is_bot: Some(true),
                ..BlockRule::default()
            }],
        };
        let mut facts = facts();
        facts.actor_is_bot = false;
        facts.actor = Some(String::from("alice"));

        assert!(matching_block_rule(&filters, &facts).is_none());
    }

    #[test]
    fn ignores_empty_rules() {
        let filters = FiltersConfig {
            block: vec![BlockRule::default()],
        };

        assert!(matching_block_rule(&filters, &facts()).is_none());
    }
}

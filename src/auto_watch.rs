use wildmatch::WildMatch;

use crate::config::AutoWatchConfig;
use crate::github::AutoWatchRepository;

pub fn should_watch_repository(
    config: &AutoWatchConfig,
    repository: &AutoWatchRepository,
    current_user: &str,
) -> bool {
    matches_any(&config.include, &repository.full_name, current_user)
        && !matches_any(&config.exclude, &repository.full_name, current_user)
}

fn matches_any(patterns: &[String], full_name: &str, current_user: &str) -> bool {
    patterns
        .iter()
        .map(|pattern| expand_me(pattern, current_user))
        .any(|pattern| WildMatch::new(&pattern).matches(full_name))
}

fn expand_me(pattern: &str, current_user: &str) -> String {
    if pattern == "@me" {
        return String::from(current_user);
    }

    pattern
        .strip_prefix("@me/")
        .map(|rest| format!("{current_user}/{rest}"))
        .unwrap_or_else(|| String::from(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(full_name: &str) -> AutoWatchRepository {
        AutoWatchRepository {
            full_name: String::from(full_name),
            html_url: format!("https://github.com/{full_name}"),
            archived: false,
            fork: false,
        }
    }

    #[test]
    fn watches_repositories_matching_default_include() {
        assert!(should_watch_repository(
            &AutoWatchConfig::default(),
            &repository("example-org/api"),
            "alice"
        ));
    }

    #[test]
    fn supports_me_as_current_user_owner_pattern() {
        let config = AutoWatchConfig {
            enabled: true,
            include: vec![String::from("@me/*")],
            exclude: Vec::new(),
        };

        assert!(should_watch_repository(
            &config,
            &repository("alice/app"),
            "alice"
        ));
        assert!(!should_watch_repository(
            &config,
            &repository("example-org/app"),
            "alice"
        ));
    }

    #[test]
    fn exclude_patterns_opt_out_after_me_expansion() {
        let config = AutoWatchConfig {
            enabled: true,
            include: vec![String::from("*/*")],
            exclude: vec![String::from("@me/noisy-*")],
        };

        assert!(!should_watch_repository(
            &config,
            &repository("alice/noisy-app"),
            "alice"
        ));
        assert!(should_watch_repository(
            &config,
            &repository("alice/quiet-app"),
            "alice"
        ));
    }
}

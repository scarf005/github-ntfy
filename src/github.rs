mod auth;
mod client;
mod model;
mod timeline;

pub use client::GitHubClient;
pub use model::{
    AutoWatchRepository, PullRequestDetails, RepositorySubscriptionResult, Thread,
    TimelineActivity, TimelineEvent,
};

#[cfg(test)]
pub use model::{Label, Owner, Repository, Subject, Team, User};

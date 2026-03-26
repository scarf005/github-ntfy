use anyhow::{Context, Result};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED,
    USER_AGENT,
};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;

use crate::config::GitHubConfig;

use super::auth::resolve_token;
use super::model::{PollResult, PullRequestDetails, Thread, TimelineEvent};
use super::timeline::timeline_url;

const API_VERSION_HEADER: HeaderName = HeaderName::from_static("x-github-api-version");
const POLL_INTERVAL_HEADER: HeaderName = HeaderName::from_static("x-poll-interval");

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

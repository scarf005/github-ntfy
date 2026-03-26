use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, StatusCode, Url};

use crate::config::NtfyConfig;
use crate::render::RenderedNotification;

const TITLE_HEADER: HeaderName = HeaderName::from_static("title");
const CLICK_HEADER: HeaderName = HeaderName::from_static("click");
const ICON_HEADER: HeaderName = HeaderName::from_static("icon");
const TAGS_HEADER: HeaderName = HeaderName::from_static("tags");
const PRIORITY_HEADER: HeaderName = HeaderName::from_static("priority");

#[derive(Clone)]
pub struct NtfyClient {
    client: Client,
    publish_url: Url,
    token: Option<String>,
}

impl NtfyClient {
    pub fn new(config: &NtfyConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .context("failed to build ntfy client")?;
        let publish_url = Url::parse(&config.publish_url).context("invalid ntfy.publish_url")?;

        Ok(Self {
            client,
            publish_url,
            token: config.token.clone(),
        })
    }

    pub async fn send(&self, notification: &RenderedNotification) -> Result<()> {
        self.client
            .post(self.publish_url.clone())
            .headers(build_headers(notification, self.token.as_deref())?)
            .body(notification.message.clone())
            .send()
            .await
            .context("failed to send ntfy request")?
            .error_for_status()
            .context("ntfy request failed")?;

        Ok(())
    }

    pub async fn check_access(&self) -> Result<()> {
        let mut request = self.client.get(self.publish_url.clone());
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }

        let response = request
            .send()
            .await
            .context("failed to reach ntfy endpoint")?;
        let status = response.status();

        if status.is_success() || status == StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }

        response
            .error_for_status()
            .context("ntfy endpoint returned an error")?;
        Ok(())
    }
}

fn build_headers(notification: &RenderedNotification, token: Option<&str>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(TITLE_HEADER, HeaderValue::from_str(&notification.title)?);
    headers.insert(
        CLICK_HEADER,
        HeaderValue::from_str(&notification.click_url)?,
    );
    headers.insert(ICON_HEADER, HeaderValue::from_str(&notification.icon_url)?);
    headers.insert(TAGS_HEADER, HeaderValue::from_str(&notification.tags)?);
    headers.insert(
        PRIORITY_HEADER,
        HeaderValue::from_str(&notification.priority.to_string())?,
    );

    if let Some(token) = token {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
    }

    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_notification() -> RenderedNotification {
        RenderedNotification {
            dedupe_key: String::from("1|now"),
            title: String::from("Title"),
            message: String::from("Body"),
            click_url: String::from("https://github.com/octo/repo/pull/1"),
            icon_url: String::from("https://avatars.githubusercontent.com/u/1?v=4"),
            tags: String::from("github,pr"),
            priority: 4,
        }
    }

    #[test]
    fn builds_ntfy_headers_without_auth_when_token_missing() {
        let headers = build_headers(&sample_notification(), None).expect("headers");

        assert_eq!(headers.get(TITLE_HEADER).expect("title"), "Title");
        assert_eq!(headers.get(TAGS_HEADER).expect("tags"), "github,pr");
        assert!(headers.get(AUTHORIZATION).is_none());
    }

    #[test]
    fn builds_ntfy_headers_with_bearer_token() {
        let headers = build_headers(&sample_notification(), Some("secret")).expect("headers");

        assert_eq!(
            headers.get(AUTHORIZATION).expect("authorization"),
            "Bearer secret"
        );
        assert_eq!(headers.get(PRIORITY_HEADER).expect("priority"), "4");
    }
}

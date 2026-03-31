use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::config::ActionsConfig;
use crate::github::GitHubClient;

#[derive(Clone)]
struct ActionState {
    github: GitHubClient,
    token: Arc<str>,
}

pub fn spawn_server(
    config: &ActionsConfig,
    github: GitHubClient,
) -> Result<JoinHandle<Result<()>>> {
    let listener = StdTcpListener::bind(&config.listen_addr)
        .with_context(|| format!("failed to bind action server on {}", config.listen_addr))?;
    listener
        .set_nonblocking(true)
        .context("failed to set action server listener to non-blocking")?;
    let listener = tokio::net::TcpListener::from_std(listener)
        .context("failed to create async action server listener")?;

    let state = ActionState {
        github,
        token: Arc::<str>::from(config.token.clone()),
    };
    let app = Router::new()
        .route("/api/actions/read/{thread_id}", post(mark_read))
        .route("/api/actions/done/{thread_id}", post(mark_done))
        .route("/api/actions/mute/{thread_id}", post(mute_thread))
        .with_state(state);
    let listen_addr = config.listen_addr.clone();

    Ok(tokio::spawn(async move {
        info!(listen_addr, "action callback server listening");
        axum::serve(listener, app)
            .await
            .context("action callback server failed")
    }))
}

pub fn notification_actions(config: &ActionsConfig, thread_id: &str) -> Option<String> {
    if !config.enabled {
        return None;
    }

    let base = config.public_base_url.trim_end_matches('/');
    let token = &config.token;

    Some(format!(
        "http, Done, {base}/api/actions/done/{thread_id}, method=POST, headers.Authorization=\"Bearer {token}\", clear=true; \
http, Read, {base}/api/actions/read/{thread_id}, method=POST, headers.Authorization=\"Bearer {token}\", clear=true; \
http, Mute, {base}/api/actions/mute/{thread_id}, method=POST, headers.Authorization=\"Bearer {token}\", clear=true"
    ))
}

async fn mark_read(
    State(state): State<ActionState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> StatusCode {
    handle_action(headers, state, thread_id, |github, thread_id| async move {
        github.mark_thread_as_read(&thread_id).await
    })
    .await
}

async fn mark_done(
    State(state): State<ActionState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> StatusCode {
    handle_action(headers, state, thread_id, |github, thread_id| async move {
        github.mark_thread_as_done(&thread_id).await
    })
    .await
}

async fn mute_thread(
    State(state): State<ActionState>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> StatusCode {
    handle_action(headers, state, thread_id, |github, thread_id| async move {
        github.ignore_thread(&thread_id).await
    })
    .await
}

async fn handle_action<F, Fut>(
    headers: HeaderMap,
    state: ActionState,
    thread_id: String,
    action: F,
) -> StatusCode
where
    F: FnOnce(GitHubClient, String) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    if !authorized(&headers, state.token.as_ref()) {
        warn!(thread_id, "rejected unauthorized action callback");
        return StatusCode::UNAUTHORIZED;
    }

    match action(state.github, thread_id.clone()).await {
        Ok(()) => {
            info!(thread_id, "completed notification action callback");
            StatusCode::NO_CONTENT
        }
        Err(error) => {
            warn!(thread_id, error = %error, "notification action callback failed");
            StatusCode::BAD_GATEWAY
        }
    }
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(expected.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorizes_matching_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer secret".parse().expect("header"));

        assert!(authorized(&headers, "secret"));
        assert!(!authorized(&headers, "wrong"));
    }

    #[test]
    fn builds_notification_actions_for_thread() {
        let actions = notification_actions(
            &ActionsConfig {
                enabled: true,
                listen_addr: String::from("127.0.0.1:8787"),
                public_base_url: String::from("http://127.0.0.1:8787/"),
                token: String::from("secret"),
            },
            "123",
        )
        .expect("actions");

        assert!(actions.contains("/api/actions/done/123"));
        assert!(actions.contains("headers.Authorization=\"Bearer secret\""));
    }
}

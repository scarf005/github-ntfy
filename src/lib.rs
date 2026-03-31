#![doc = r#"
GitHub notification forwarder for `ntfy`.

The crate polls the GitHub notifications inbox, enriches pull request and issue
updates with extra context, applies local block rules, and publishes the result
to an `ntfy` topic.

Key components:

- `app`: polling loop and orchestration
- `config`: config loading and validation
- `github`: GitHub REST and GraphQL clients
- `ntfy`: `ntfy` publish client
- `render`: notification title/body rendering
- `filter`: rule-based suppression
"#]

pub mod app;
pub mod config;
pub mod filter;
pub mod github;
pub mod ntfy;
pub mod render;
pub mod state;

pub use app::App;
pub use config::LoadedConfig;

pub(crate) mod action;

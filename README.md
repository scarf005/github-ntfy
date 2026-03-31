# github-ntfy

Self-hosted GitHub notification forwarder for `ntfy`.

- Rust version: `github-ntfy-agent` as a long-running service
- PRs are enriched from GraphQL commit/timeline data and issues from GitHub timeline activity
- Repeated updates for the same GitHub thread replace the existing `ntfy` notification
- Optional `Done`, `Read`, and `Mute` action buttons can call back into the agent
- Rust config supports block rules such as repo/org/bot filters

## Why

- GitHub mobile notifications are noisy and low-context
- `ntfy` gives better routing, priority, filtering, and self-hosting

## Quick start

```bash
cargo run -- check --config ./config.example.toml
cargo run -- once --config ./config.example.toml
cargo run -- run --config ./config.example.toml
```

`github.token` is optional if `gh auth` is already logged in.

## Docs

- `cargo doc --no-deps` builds local API docs
- GitHub Pages publishes rustdoc from `.github/workflows/rustdoc-pages.yml`
- After deployment, the docs site lives at `https://scarf005.github.io/github-ntfy/`

Config:

- user install: `~/.config/github-ntfy-agent/config.toml`
- system install: `/etc/github-ntfy-agent/config.toml`
- state file defaults to the platform state directory unless `app.state_path` is set
- action callbacks can be enabled with `[actions]` using a reachable `public_base_url`

Linux install:

```bash
./install-rust-agent.sh
systemctl --user enable --now github-ntfy-agent.service
```

System service:

```bash
sudo systemctl enable --now github-ntfy-agent.service
```

## Filter rules

```toml
[filters]

[[filters.block]]
name = "ignore example-org bots"
repo = "example-org/*"
actor_is_bot = true

[[filters.block]]
name = "ignore issue label churn"
owner = "example-org"
subject_type = "Issue"
activity = "labeled"
```

Supported fields:

- `repo`
- `owner`
- `title`
- `actor`
- `actor_is_bot`
- `reason`
- `subject_type`
- `activity`

## Action Buttons

```toml
[actions]
enabled = true
listen_addr = "0.0.0.0:8787"
public_base_url = "http://100.127.86.108:8787"
token = "replace-with-random-shared-secret"
```

- When enabled, PR and issue notifications include `Done`, `Read`, and `Mute` buttons
- The phone or desktop client calls the agent callback server, and the agent performs the GitHub notification API request with its own credentials
- `public_base_url` must be reachable by the device tapping the `ntfy` action button

## Notes

- Notifications API can use the current `gh auth` session too; classic PAT is still the easiest portable setup
- Android tap uses `Click` so the relevant GitHub page opens directly
- Unknown notification types fall back to the repository page

# github-ntfy

Self-hosted GitHub notification forwarder for `ntfy`.

- Rust version: `github-ntfy-agent` as a long-running service
- PR and issue titles are enriched from GitHub timeline activity
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

Config:

- user install: `~/.config/github-ntfy-agent/config.toml`
- system install: `/etc/github-ntfy-agent/config.toml`
- state file defaults to the platform state directory unless `app.state_path` is set

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
- `actor`
- `actor_is_bot`
- `reason`
- `subject_type`
- `activity`

## Notes

- Notifications API can use the current `gh auth` session too; classic PAT is still the easiest portable setup
- Android tap uses `Click` so the relevant GitHub page opens directly
- Unknown notification types fall back to the repository page

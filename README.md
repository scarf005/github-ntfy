# github-ntfy

Self-hosted GitHub notification forwarder for `ntfy`.

- Shell version: `bin/github-ntfy` via `systemd` timer
- Rust version: `github-ntfy-agent` as a long-running service
- PR and issue titles are enriched from GitHub timeline activity
- Rust config supports block rules such as repo/org/bot filters

## Why

- GitHub mobile notifications are noisy and low-context
- `ntfy` gives better routing, priority, filtering, and self-hosting

## Shell quick start

```bash
sudo ./install.sh
sudo systemctl enable --now github-ntfy.timer
journalctl -u github-ntfy.service -f
```

Config: `/etc/github-ntfy.env`

- `GH_TOKEN`: classic PAT, or use existing `gh auth`
- `NTFY_URL`: publish endpoint such as `http://host:8080/github`

## Rust quick start

```bash
cargo run -- check --config ./config.example.toml
cargo run -- once --config ./config.example.toml
cargo run -- run --config ./config.example.toml
```

Linux install:

```bash
./install-rust-agent.sh
systemctl --user enable --now github-ntfy-agent.service
```

System service:

```bash
sudo systemctl enable --now github-ntfy-agent.service
```

## Rust filter rules

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

- Notifications API currently means classic PAT is the practical auth model
- Android tap uses `Click` so the relevant GitHub page opens directly
- Unknown notification types fall back to the repository page

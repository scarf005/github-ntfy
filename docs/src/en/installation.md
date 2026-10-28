# Installation

Requires a Rust toolchain (edition 2024).

## User service (Linux)

```bash
./install-rust-agent.sh
systemctl --user enable --now github-ntfy-agent.service
```

- Binary: `~/.local/bin/github-ntfy-agent`
- Config: `~/.config/github-ntfy-agent/config.toml` (copied from `config.example.toml` if missing)
- Unit: `systemd/user/github-ntfy-agent.service`

Check it:

```bash
systemctl --user status github-ntfy-agent.service
journalctl --user -u github-ntfy-agent.service -f
```

## System service

```bash
sudo ./install-rust-agent.sh
sudo systemctl enable --now github-ntfy-agent.service
```

- Binary: `/usr/local/bin/github-ntfy-agent`
- Config: `/etc/github-ntfy-agent/config.toml`
- State: `/var/lib/github-ntfy-agent`, owned by the `github-ntfy` system user

## From source

```bash
cargo run -- check --config ./config.example.toml
cargo run -- run --config ./config.example.toml
```

## Authentication

`github.token` is optional when `gh auth login` has a session. A classic personal access token with the `notifications` and `repo` scopes is the most portable setup.

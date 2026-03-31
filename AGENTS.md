## Local Rules

- After changing Rust source, systemd units, or install scripts in this repo, rebuild and redeploy the user service before reporting completion.
- Required verification/deploy sequence for this repo:
  1. `cargo fmt`
  2. `cargo test`
  3. `cargo build --release`
  4. `./install-rust-agent.sh`
  5. `systemctl --user restart github-ntfy-agent.service`
  6. `systemctl --user --no-pager --full status github-ntfy-agent.service`

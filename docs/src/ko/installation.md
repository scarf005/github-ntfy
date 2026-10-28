# 설치

Rust 툴체인(edition 2024)이 필요하다.

## 사용자 서비스 (Linux)

```bash
./install-rust-agent.sh
systemctl --user enable --now github-ntfy-agent.service
```

- 바이너리: `~/.local/bin/github-ntfy-agent`
- 설정: `~/.config/github-ntfy-agent/config.toml` (없으면 `config.example.toml`을 복사)
- 유닛: `systemd/user/github-ntfy-agent.service`

상태 확인:

```bash
systemctl --user status github-ntfy-agent.service
journalctl --user -u github-ntfy-agent.service -f
```

## 시스템 서비스

```bash
sudo ./install-rust-agent.sh
sudo systemctl enable --now github-ntfy-agent.service
```

- 바이너리: `/usr/local/bin/github-ntfy-agent`
- 설정: `/etc/github-ntfy-agent/config.toml`
- 상태: `/var/lib/github-ntfy-agent` (`github-ntfy` 시스템 사용자 소유)

## 소스에서 실행

```bash
cargo run -- check --config ./config.example.toml
cargo run -- run --config ./config.example.toml
```

## 인증

`gh auth login` 세션이 있으면 `github.token`은 생략할 수 있다. 어디서나 쓰기 쉬운 방법은 `notifications`, `repo` 스코프를 가진 classic personal access token이다.

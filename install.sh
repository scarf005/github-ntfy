#!/usr/bin/env bash
set -euo pipefail

readonly APP_NAME="github-ntfy"

install_system() {
  readonly APP_USER="github-ntfy"
  readonly BIN_DST="/usr/local/bin/github-ntfy"
  readonly ENV_DST="/etc/github-ntfy.env"
  readonly SERVICE_DST="/etc/systemd/system/github-ntfy.service"
  readonly TIMER_DST="/etc/systemd/system/github-ntfy.timer"

  install -d -m 0755 /usr/local/bin
  install -m 0755 bin/github-ntfy "$BIN_DST"

  if ! getent group "$APP_USER" >/dev/null 2>&1; then
    groupadd --system "$APP_USER"
  fi

  if ! id -u "$APP_USER" >/dev/null 2>&1; then
    useradd --system --gid "$APP_USER" --home-dir /var/lib/github-ntfy --create-home --shell /usr/sbin/nologin "$APP_USER"
  fi

  install -d -m 0750 -o "$APP_USER" -g "$APP_USER" /var/lib/github-ntfy

  if [[ ! -f "$ENV_DST" ]]; then
    install -m 0600 github-ntfy.env.example "$ENV_DST"
  fi

  install -m 0644 systemd/github-ntfy.service "$SERVICE_DST"
  install -m 0644 systemd/github-ntfy.timer "$TIMER_DST"

  systemctl daemon-reload

  printf 'installed %s\n' "$BIN_DST"
  printf 'edit %s, then enable the timer:\n' "$ENV_DST"
  printf '  systemctl enable --now github-ntfy.timer\n'
}

install_user() {
  readonly BIN_DST="${HOME}/.local/bin/github-ntfy"
  readonly ENV_DST="${HOME}/.config/github-ntfy.env"
  readonly SERVICE_DST="${HOME}/.config/systemd/user/github-ntfy.service"
  readonly TIMER_DST="${HOME}/.config/systemd/user/github-ntfy.timer"

  install -d -m 0755 "${HOME}/.local/bin"
  install -m 0755 bin/github-ntfy "$BIN_DST"

  install -d -m 0755 "${HOME}/.config"
  install -d -m 0755 "${HOME}/.config/systemd/user"
  install -d -m 0755 "${HOME}/.local/state/github-ntfy"

  if [[ ! -f "$ENV_DST" ]]; then
    cat >"$ENV_DST" <<'EOF'
GH_TOKEN=ghp_replace_me
NTFY_URL=http://100.127.86.108:8080/github
# NTFY_TOKEN=tk_replace_me
# PARTICIPATING=false
# PER_PAGE=100
# MAX_PAGES=10
EOF
    chmod 0600 "$ENV_DST"
  fi

  install -m 0644 systemd/user/github-ntfy.service "$SERVICE_DST"
  install -m 0644 systemd/user/github-ntfy.timer "$TIMER_DST"

  systemctl --user daemon-reload

  printf 'installed %s\n' "$BIN_DST"
  printf 'edit %s, then enable the timer:\n' "$ENV_DST"
  printf '  systemctl --user enable --now github-ntfy.timer\n'
}

if [[ "${EUID}" -eq 0 ]]; then
  install_system
else
  install_user
fi

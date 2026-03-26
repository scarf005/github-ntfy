#!/usr/bin/env bash
set -euo pipefail

readonly APP_NAME="github-ntfy-agent"

build_binary() {
  cargo build --release
}

install_system() {
  local bin_dst="/usr/local/bin/${APP_NAME}"
  local config_dir="/etc/${APP_NAME}"
  local config_dst="${config_dir}/config.toml"
  local service_dst="/etc/systemd/system/${APP_NAME}.service"
  local state_dir="/var/lib/${APP_NAME}"

  install -d -m 0755 /usr/local/bin
  install -m 0755 "target/release/${APP_NAME}" "$bin_dst"

  if ! getent group github-ntfy >/dev/null 2>&1; then
    groupadd --system github-ntfy
  fi

  if ! id -u github-ntfy >/dev/null 2>&1; then
    useradd --system --gid github-ntfy --home-dir /var/lib/${APP_NAME} --create-home --shell /usr/sbin/nologin github-ntfy
  fi

  install -d -m 0750 -o github-ntfy -g github-ntfy "$state_dir"
  install -d -m 0750 -o github-ntfy -g github-ntfy "$state_dir/.local/state/${APP_NAME}"
  install -d -m 0755 "$config_dir"

  if [[ ! -f "$config_dst" ]]; then
    install -m 0600 config.example.toml "$config_dst"
  fi

  install -m 0644 systemd/${APP_NAME}.service "$service_dst"
  systemctl daemon-reload

  printf 'installed %s\n' "$bin_dst"
  printf 'edit %s, then enable the service:\n' "$config_dst"
  printf '  systemctl enable --now %s.service\n' "$APP_NAME"
}

install_user() {
  local bin_dir="${HOME}/.local/bin"
  local bin_dst="${bin_dir}/${APP_NAME}"
  local config_dir="${HOME}/.config/${APP_NAME}"
  local config_dst="${config_dir}/config.toml"
  local service_dir="${HOME}/.config/systemd/user"
  local service_dst="${service_dir}/${APP_NAME}.service"

  install -d -m 0755 "$bin_dir"
  install -m 0755 "target/release/${APP_NAME}" "$bin_dst"

  install -d -m 0755 "$config_dir"
  install -d -m 0755 "$service_dir"
  install -d -m 0755 "${HOME}/.local/state/${APP_NAME}"

  if [[ ! -f "$config_dst" ]]; then
    install -m 0600 config.example.toml "$config_dst"
  fi

  install -m 0644 systemd/user/${APP_NAME}.service "$service_dst"
  systemctl --user daemon-reload

  printf 'installed %s\n' "$bin_dst"
  printf 'edit %s, then enable the service:\n' "$config_dst"
  printf '  systemctl --user enable --now %s.service\n' "$APP_NAME"
}

build_binary

if [[ "${EUID}" -eq 0 ]]; then
  install_system
else
  install_user
fi

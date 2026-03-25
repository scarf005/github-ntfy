# github-ntfy

Tiny Linux-only GitHub notification forwarder for `ntfy`.

It runs from `systemd` timer, polls GitHub notifications with `gh api /notifications` every minute by default, deduplicates by notification thread and update time, and publishes to `ntfy` with the repository owner's GitHub avatar plus a `Click` header so tapping the Android notification opens the relevant GitHub page. Pull request notifications also use the latest timeline activity GitHub exposes to enrich the title and description.

## Requirements

- Linux with `systemd`
- `bash`
- `gh`
- `jq`
- `curl`

## Setup

1. Create a classic GitHub personal access token.
2. Copy `github-ntfy.env.example` to `/etc/github-ntfy.env`
3. Fill in `NTFY_URL` and optionally `GH_TOKEN`
4. Run `sudo ./install.sh`
5. Enable the timer:

```bash
sudo systemctl enable --now github-ntfy.timer
```

6. Check logs:

```bash
journalctl -u github-ntfy.service -f
```

## Config

`/etc/github-ntfy.env`

- `GH_TOKEN`: optional classic PAT used by `gh api`; if omitted, existing `gh auth` login is used
- `NTFY_URL`: full publish URL, for example `http://100.127.86.108:8080/github`
- `NTFY_TOKEN`: optional bearer token for private `ntfy`
- `PARTICIPATING`: optional, `true` to limit to participating notifications
- `PER_PAGE`: optional, default `100`
- `MAX_PAGES`: optional, default `10`
- `STATE_DIR`: optional, default `/var/lib/github-ntfy`
- `NTFY_TIMEOUT`: optional, default `5`

## Manual run

```bash
sudo systemctl start github-ntfy.service
```

Or directly:

```bash
sudo GH_TOKEN=... NTFY_URL=http://100.127.86.108:8080/github ./bin/github-ntfy poll
```

## How links work

The script converts GitHub API subject URLs into GitHub web URLs and sends them as the `Click` header. On Android, tapping the `ntfy` notification opens that GitHub page.

Current mappings cover:

- issues
- pull requests
- commits
- discussions
- releases

Unknown notification types fall back to the repository page.

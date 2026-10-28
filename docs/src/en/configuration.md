# Configuration

The config file is TOML. See [`config.example.toml`](https://github.com/scarf005/github-ntfy/blob/main/config.example.toml).

## `[github]`

| Key | Default | Meaning |
| --- | --- | --- |
| `token` | `gh auth` session | GitHub token |
| `api_base` | `https://api.github.com` | API root |
| `participating` | `false` | Only threads you participate in |
| `per_page` | `100` | Page size, 1–100 |
| `timeout_secs` | `5` | Request timeout |
| `enrich_pull_requests` | `true` | Fetch PR details and timeline |
| `enrich_issues` | `true` | Fetch issue timeline |

## `[ntfy]`

| Key | Default | Meaning |
| --- | --- | --- |
| `publish_url` | required | Topic URL, e.g. `http://ntfy.example/github` |
| `token` | none | ntfy access token |
| `timeout_secs` | `5` | Request timeout |

## `[app]`

| Key | Default | Meaning |
| --- | --- | --- |
| `poll_interval_secs` | `60` | Poll interval; GitHub's `X-Poll-Interval` can raise it |
| `max_seen` | `2000` | Remembered notification keys |
| `log_level` | `info` | `tracing` filter |
| `state_path` | platform state dir | `state.json` location |

## `[auto_watch]`

GitHub no longer watches newly accessible repositories. The agent subscribes to repositories that appear after its first scan. The first scan records existing repositories as a baseline and changes nothing. Repositories with an explicit subscription setting are skipped.

```toml
[auto_watch]
enabled = true
include = ["*/*"]
exclude = ["@me/noisy-*", "example-org/archive-*"]
```

`@me` expands to the authenticated login.

## `[filters]`

Each `[[filters.block]]` drops notifications matching all of its fields. `repo`, `owner`, `title`, and `actor` accept `*` and `?` wildcards; `reason`, `subject_type`, and `activity` match case-insensitively.

```toml
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

Fields: `repo`, `owner`, `title`, `actor`, `actor_is_bot`, `reason`, `subject_type`, `activity`.

`activity` values: `commented`, `reviewed`, `review_approved`, `review_changes_requested`, `committed`, `merged`, `closed`, `closed_via_pull_request`, `reopened`, `assigned`, `unassigned`, `labeled`, `unlabeled`, `review_requested`, `review_request_removed`, and other GitHub timeline event names as is. `closed` also matches `closed_via_pull_request`.

When a rule with `activity` blocks the first activity on a new issue or pull request, the agent sends the opening body instead.

## `[actions]`

```toml
[actions]
enabled = true
listen_addr = "0.0.0.0:8787"
public_base_url = "http://100.64.0.1:8787"
token = "replace-with-random-shared-secret"
```

- Adds `Done`, `Read`, and `Mute` buttons to notifications.
- The device that taps a button calls `POST {public_base_url}/api/actions/{done,read,mute}/{thread_id}` with `Authorization: Bearer {token}`; the agent then calls GitHub with its own credentials.
- `public_base_url` must be reachable from that device. `listen_addr` defaults to `127.0.0.1:8787`.

# 설정

설정 파일은 TOML이다. [`config.example.toml`](https://github.com/scarf005/github-ntfy/blob/main/config.example.toml)을 참고한다.

## `[github]`

| 키 | 기본값 | 의미 |
| --- | --- | --- |
| `token` | `gh auth` 세션 | GitHub 토큰 |
| `api_base` | `https://api.github.com` | API 루트 |
| `participating` | `false` | 참여 중인 스레드만 |
| `per_page` | `100` | 페이지 크기, 1–100 |
| `timeout_secs` | `5` | 요청 타임아웃 |
| `enrich_pull_requests` | `true` | PR 상세·타임라인 조회 |
| `enrich_issues` | `true` | 이슈 타임라인 조회 |

## `[ntfy]`

| 키 | 기본값 | 의미 |
| --- | --- | --- |
| `publish_url` | 필수 | 토픽 URL (예: `http://ntfy.example/github`) |
| `token` | 없음 | ntfy 액세스 토큰 |
| `timeout_secs` | `5` | 요청 타임아웃 |

## `[app]`

| 키 | 기본값 | 의미 |
| --- | --- | --- |
| `poll_interval_secs` | `60` | 폴링 간격. GitHub의 `X-Poll-Interval`이 더 길면 그 값을 따른다 |
| `max_seen` | `2000` | 기억하는 알림 키 수 |
| `log_level` | `info` | `tracing` 필터 |
| `state_path` | 플랫폼 상태 디렉터리 | `state.json` 위치 |

## `[auto_watch]`

GitHub는 새로 접근 가능해진 저장소를 더 이상 자동으로 watch하지 않는다. 에이전트는 첫 스캔 뒤에 나타난 저장소를 구독한다. 첫 스캔은 기존 저장소를 기준선으로 기록만 하고 아무것도 바꾸지 않는다. 구독 설정이 명시된 저장소는 건너뛴다.

```toml
[auto_watch]
enabled = true
include = ["*/*"]
exclude = ["@me/noisy-*", "example-org/archive-*"]
```

`@me`는 인증된 로그인으로 바뀐다.

## `[filters]`

`[[filters.block]]` 하나는 모든 필드가 맞는 알림을 버린다. `repo`, `owner`, `title`, `actor`는 `*`, `?` 와일드카드를 쓸 수 있고, `reason`, `subject_type`, `activity`는 대소문자 구분 없이 비교한다.

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

필드: `repo`, `owner`, `title`, `actor`, `actor_is_bot`, `reason`, `subject_type`, `activity`.

`activity` 값: `commented`, `reviewed`, `review_approved`, `review_changes_requested`, `committed`, `merged`, `closed`, `closed_via_pull_request`, `reopened`, `assigned`, `unassigned`, `labeled`, `unlabeled`, `review_requested`, `review_request_removed`, 그 밖의 GitHub 타임라인 이벤트 이름 그대로. `closed`는 `closed_via_pull_request`도 포함한다.

새 이슈나 PR의 첫 활동이 `activity` 규칙에 막히면 대신 본문 알림을 보낸다.

## `[actions]`

```toml
[actions]
enabled = true
listen_addr = "0.0.0.0:8787"
public_base_url = "http://100.64.0.1:8787"
token = "replace-with-random-shared-secret"
```

- 알림에 `Done`, `Read`, `Mute` 버튼을 붙인다.
- 버튼을 누른 기기가 `Authorization: Bearer {token}`으로 `POST {public_base_url}/api/actions/{done,read,mute}/{thread_id}`를 호출하면, 에이전트가 자기 자격 증명으로 GitHub API를 호출한다.
- `public_base_url`은 그 기기에서 접근 가능해야 한다. `listen_addr` 기본값은 `127.0.0.1:8787`이다.

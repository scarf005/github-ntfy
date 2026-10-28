# 개요

`github-ntfy-agent`는 GitHub 알림 API를 폴링해 스레드마다 ntfy 토픽으로 알림을 보낸다.

- PR은 GraphQL 커밋·타임라인 데이터로, 이슈는 REST 타임라인으로 내용을 보강한다.
- GitHub 스레드 하나는 ntfy 알림 하나에 대응한다. 새 활동이 오면 알림을 교체하고, 이전 활동은 최신 줄 아래에 남긴다.
- 타임라인을 가져온 경우(`enrich_issues`, `enrich_pull_requests`) 새 이슈나 PR은 본문으로 보여준다.
- 선택적으로 `Done`, `Read`, `Mute` 버튼을 붙여 에이전트로 콜백할 수 있다.
- 새로 접근 가능해진 저장소를 자동으로 watch한다.
- 차단 규칙으로 저장소, 소유자, 작성자, reason, 대상 종류, 활동, 제목별로 알림을 버린다.

GitHub Mobile은 받은편지함 푸시를 자주 빠뜨린다([커뮤니티 토론](https://github.com/orgs/community/discussions/7229)). ntfy는 라우팅, 우선순위, 셀프 호스팅을 제공한다.

## 명령

| 명령 | 동작 |
| --- | --- |
| `github-ntfy-agent check` | GitHub·ntfy 접근 확인 |
| `github-ntfy-agent once` | 한 번 폴링하고 종료 |
| `github-ntfy-agent run` | 계속 폴링 |

모든 명령은 `--config <경로>`를 받는다.

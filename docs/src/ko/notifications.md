# 알림

## 형태

- 제목: 대상 제목과 번호 (예: `Fix pull link (#42)`).
- 본문: 최신 활동이 맨 위, 그 아래에 같은 스레드의 이전 활동.
- 클릭: 이슈, PR, 커밋, 릴리스 페이지를 연다. 그 밖의 종류는 저장소 페이지를 연다.
- 우선순위: 보안 알림 `5`, 멘션과 리뷰 요청 `4`, 나머지 `3`.

## 활동 줄

| 활동 | 줄 |
| --- | --- |
| 댓글 | `@user: 댓글 내용` |
| 나를 멘션 | `@user mentioned you` 다음 줄에 댓글 |
| 리뷰 | `@user approved`, `@user requested changes`, `@user reviewed` |
| 커밋 | `@user pushed 3 commits` |
| 상태 | `@user merged`, `@user closed: 사유`, `@user reopened` |
| PR로 이슈 닫힘 | `@user closed via #123 (PR 제목)` |

## 멘션

GitHub는 한 번 멘션된 스레드의 reason을 계속 `mention`으로 둔다. 그래서 reason만으로는 최신 댓글이 나를 멘션했는지 알 수 없다. 에이전트는 다음 조건일 때만 `mentioned you`를 붙인다.

- 댓글 작성자가 인증된 사용자 본인이 아니고,
- `mention`이면 댓글 본문에 `@내-로그인`이 있을 때, `team_mention`이면 다른 사람의 댓글일 때.

나머지는 일반 댓글로 보여준다.

## 갱신

스레드마다 고정된 ntfy sequence ID를 쓰므로 새 활동이 이전 알림을 교체한다. 최신 줄과 같은 갱신은 다시 보내지 않는다. 기록이 길어지면 생략 표시와 함께 잘라낸다.

읽음 처리된 스레드도 이미 보낸 알림은 계속 갱신한다.

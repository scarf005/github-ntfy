# Notifications

## Shape

- Title: subject title and number, e.g. `Fix pull link (#42)`.
- Body: the latest activity, newest first, followed by earlier activity on the same thread.
- Click: opens the issue, pull request, commit, or release page; other types open the repository.
- Priority: `5` for security alerts, `4` for mentions and review requests, `3` otherwise.

## Activity lines

| Activity | Line |
| --- | --- |
| Comment | `@user: comment text` |
| Mention of you | `@user mentioned you` followed by the comment |
| Review | `@user approved`, `@user requested changes`, `@user reviewed` |
| Commits | `@user pushed 3 commits` |
| State | `@user merged`, `@user closed: reason`, `@user reopened` |
| Issue closed by PR | `@user closed via #123 (PR title)` |

## Mentions

GitHub keeps a thread's reason as `mention` after the first mention, so the reason alone does not say whether the latest comment mentions you. The agent labels a comment `mentioned you` only when:

- the comment author is not the authenticated user, and
- for `mention`, the comment body contains `@your-login`; for `team_mention`, any other author's comment counts.

Other comments render as plain comments.

## Updates

Each thread uses a stable ntfy sequence ID, so new activity replaces the previous notification. An update identical to the newest line is not sent again. Long histories are trimmed with an omitted-lines marker.

Read threads still update notifications that were already sent.

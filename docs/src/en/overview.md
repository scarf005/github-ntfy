# Overview

`github-ntfy-agent` polls the GitHub notifications API and publishes each thread to an ntfy topic.

- Pull requests are enriched from GraphQL commit and timeline data; issues from the REST timeline.
- Every GitHub thread maps to one ntfy notification. New activity replaces it and keeps earlier lines below the newest one.
- A new issue or pull request is shown with its opening body when its timeline is fetched (`enrich_issues`, `enrich_pull_requests`).
- Optional `Done`, `Read`, and `Mute` buttons call back into the agent.
- Newly accessible repositories are watched automatically.
- Block rules drop notifications by repository, owner, actor, reason, subject type, activity, or title.

GitHub Mobile skips many inbox pushes ([community discussion](https://github.com/orgs/community/discussions/7229)); ntfy gives routing, priority, and self-hosting.

## Commands

| Command | Effect |
| --- | --- |
| `github-ntfy-agent check` | Verify GitHub and ntfy access |
| `github-ntfy-agent once` | Poll once and exit |
| `github-ntfy-agent run` | Poll forever |

All commands accept `--config <path>`.

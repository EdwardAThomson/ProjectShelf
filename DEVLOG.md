# Dev Log

## 2026-07-14

Sketched the next direction for ProjectShelf on the roadmap: integrating it with automation-center, the local command-center for automations (blog-radar ideas, nightly dev-log and docs-sweep runs, open PRs, publish queue). The idea is that the shelf should show not just a project's git and roadmap state but its automation activity too: a per-project "Automation" panel or tab, and a global dashboard/export that merges the project list with automation-center's status output. No code yet; this is a backlog entry so it stays out of the headline task counts.

**Decisions & notes:** The main open question recorded is the integration seam: read automation-center's JSON/state files directly (e.g. `~/blog-ideas/ideas.json`) versus shelling out to `automation-center --status`. Deliberately placed under a Backlog heading so the roadmap parser excludes it from done/total progress.

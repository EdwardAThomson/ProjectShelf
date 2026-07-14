# Roadmap — ProjectShelf

_Status: active · updated 2026-07-14_

A local-first Linux desktop app (Rust + egui) that scans your projects directory for git repos and marked projects and presents them as a fast, searchable shelf — git metadata, language breakdowns, roadmap progress, health checks, notes, and tags, all cached in SQLite for instant startup.

## Shipped (M1–M10)

- [x] **M1** — Project discovery + cached list
- [x] **M2** — Git metadata + activity sorting
- [x] **M3** — Language detection with byte counts
- [x] **M4** — Milestones + notes per project _(manual milestones later removed; see M10)_
- [x] **M5** — GitHub remote integration
- [x] **M6** — Tags, pinned projects, YAML import, settings
- [x] **M7** — Health checks (README/license/tests/CI presence)
- [x] **M8** — Export report (Markdown/CSV)
- [x] **M9** — File watcher for incremental updates
- [x] **M10** — Auto task tracking from a canonical `ROADMAP.md` (replaces manual milestones)

## Recent improvements

- [x] Tabbed details panel (Overview / Roadmap / Notes); Pin moved to the header
- [x] Roadmap tab: uncompleted items first, completed split into a collapsed group, progress bar
- [x] Project list polish: language-tinted icons, rich hover tooltips, progress-coded roadmap badge
- [x] Lightweight file watcher (per-project, non-recursive) — fixes the crash from recursively watching ~100k dirs
- [x] Project-list performance: cached rows + no per-frame DB queries (smoother scrolling)
- [x] Language-detection fixes: exclude JSON as primary; skip CMake `.d`/`*.o.d` deps and vendored/`archive` dirs

## Next

- [ ] Project templates + init (scaffold a new project: dir, `.projman` marker, optional `git init`)
- [ ] GitHub issues / PR summary per project (needs API token + rate-limit handling)

## Backlog

- [ ] Plugin system (custom detectors)
- [ ] Per-section progress in the Roadmap tab (sections are grouped + split done/open; per-section done/total counts still TODO)
- [ ] Optional per-project roadmap-source override in `.projman/project.yaml`
- [ ] Galley/virtualized rendering for very large roadmaps (only if big lists prove laggy in practice)

### Automation-center integration (idea)

Surface reports from `automation-center` (the local automations command-center: blog-radar ideas, nightly-sweep dev-log/docs-sweep, open PRs, publish queue) inside ProjectShelf, so the shelf shows not just a project's git/roadmap state but its automation activity too.

- [ ] Per-project "Automation" panel/tab: show that project's automation-center activity (e.g. blog-radar ideas sourced from it, recent sweep PRs, open PRs) next to its git + roadmap info
- [ ] Global automation report: combine ProjectShelf's project list with automation-center's status output (timers, open PRs, publish queue) into one dashboard/export
- [ ] Decide the integration seam: read automation-center's JSON/state directly (e.g. `~/blog-ideas/ideas.json`) vs shelling out to `automation-center --status` / `automation-status`

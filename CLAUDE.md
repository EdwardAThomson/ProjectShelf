# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

ProjectShelf is a local-first Linux desktop app (Rust + egui) that scans `~/Projects` for git repos / marked projects and presents them in a browsable, searchable list with git metadata, language breakdowns, roadmap progress (from `ROADMAP.md`), notes, and tags. State is cached in a local SQLite database for instant startup.

## Commands

```bash
# Dev run (debug build) — this is the binary target
cargo run -p projectshelf_ui --bin projectshelf

# Release build, then run
cargo build --release
./target/release/projectshelf

# Check / lint
cargo check
cargo clippy

# Tests (only the checklist parser has them so far)
cargo test -p projectshelf_core
```

The only tests in the repo are unit tests for the markdown checklist parser (`checklist.rs`). `cargo test` otherwise runs ~zero tests.

System deps may be required to build (GTK/X11/SSL) — see README "Linux Dependencies".

## Naming caveat (important)

The crates were renamed from `projman_*` to `projectshelf_*`, but the on-disk artifacts kept the old `projman` name. Don't "fix" these to match the crate names — they are the real, working paths:

- Data dir: `~/.local/share/projman/`
- Database: `~/.local/share/projman/projman.sqlite`
- Settings: `~/.local/share/projman/settings.toml`
- Per-project marker / YAML: `<project>/.projman/project.yaml`

A directory is treated as a project if it contains `.git/` **or** `.projman/project.yaml`.

## Architecture

Two-crate Cargo workspace (edition 2024):

- **`crates/projectshelf_core`** — pure logic library, no UI deps. `lib.rs` re-exports every module flat (`pub use ...::*`), so the UI imports everything as `projectshelf_core::{...}`.
- **`crates/projectshelf_ui`** — the `projectshelf` binary. All UI lives in `app.rs` (one large `ProjectShelfApp` impl); `main.rs` is just the eframe bootstrap.

### Scan pipeline (`discover.rs`)

`scan_projects(root)` → `discover_projects` (recursive, capped at `MAX_DEPTH = 4`, skips `IGNORED_DIRS` from `config.rs`; stops descending once a project marker is found) → `build_project` per hit, which fans out to `git.rs`, `fs_activity.rs`, and `languages.rs`. Returns `Vec<(Project, LanguageBreakdown)>`.

`project_id` is the **SHA-256 of the absolute path string** (`generate_project_id`). It is the primary key everywhere — moving a project's directory creates a new id.

### Threading model (`app.rs`)

Scanning runs on a `thread::spawn`ed background thread communicating results back over an `std::mpsc` channel. `start_scan()` launches it; `check_scan_results()` (called each `update`) drains the channel via `try_recv()`, writes results to SQLite, and imports YAML. The UI never blocks on a scan. (Note: a `scan_tx` trigger channel is wired but currently unused.)

A second background thread is the **file watcher** (`spawn_watcher`, started once in `new`). It uses the `notify` crate, but **deliberately does not watch the projects root recursively** — that would register one inotify watch per directory (~100k on a real `~/Projects`, ~76% of them inside `node_modules`/`target`/`.git`), a huge resource cost and a likely crash trigger under FS bursts. Instead it discovers projects (`discover_projects`) and adds a **non-recursive** watch on each project's **root dir and its `.git`** (~2 watches/project). It debounces bursts (600ms quiet window), maps changed paths to their owning project dir via `find_project_root` (skipping `IGNORED_DIRS` but keeping `.git` so commit/dirty changes register), re-scans only those dirs with `scan_single_project`, and pushes them over a separate channel. `check_watch_results()` (also called each `update`) merges them into the in-memory list + DB without a full rescan. **Trade-off:** edits deep inside a project (e.g. `src/`) don't auto-refresh — only project-root changes, commits, and dirty-state changes do; use Refresh for a full re-scan. The watcher binds the project set + root at startup, so newly-added projects / a changed root only take effect after a restart (manual Refresh still uses the new root).

### Persistence (`db.rs`)

Plain `rusqlite` (bundled SQLite), no ORM/migration framework. Schema is created idempotently with `CREATE TABLE IF NOT EXISTS` in `init_schema()`. **Schema changes to existing tables are done with manual `ALTER TABLE ... ADD COLUMN` calls whose errors are ignored** (see the `github_url` migration) — follow that pattern for new columns rather than introducing a migration library. Tables: `projects`, `languages`, `user_meta`, `milestones`, `tags`, `scan_state`.

### YAML sync (`projfile.rs`)

`.projman/project.yaml` is a two-way store for **notes** (and `pinned`). On scan, `ProjectFile::load` imports notes into the DB only if the DB has none (DB is otherwise the source of truth). Editing notes in the UI calls `save_project_file`, which loads the existing YAML, updates `notes`/`pinned`, and writes it back (preserving any legacy `milestones:` block in the file). Dates use a **hand-rolled epoch-day calendar** (`parse_date`/`format_date`, `YYYY-MM-DD`) — there is no `chrono`/`time` dependency, so add date logic there, not via a new crate.

Note: the old **manual milestones** feature (M4) was removed from the UI. The `milestones` DB table, the `Milestone`/`MilestoneStatus` models, and `ProjectFile`'s milestone fields still exist in `projectshelf_core` (dormant, not surfaced) — milestone tracking is now done via the read-only roadmap parser below.

### Git & external integrations

`git.rs` shells out to the `git` CLI via `std::process::Command` (no `git2`/libgit2) — branch, dirty status, last-commit timestamp, and remote URL parsed into a GitHub web URL. Quick actions in `app.rs` also shell out: `xdg-open` (folder), terminal emulators, and a fallback chain of IDE binaries (the user's configured `preferred_ide` first, then `code`/`cursor`/`windsurf`/...).

### Settings

`settings.toml` is read/written with **manual string formatting and line parsing** in `app.rs` (`load_app_settings`/`save_app_settings`), not a TOML parser. Keys: `projects_root`, `preferred_ide`.

## Reference docs & roadmap

`docs/spec_and_plan.md` is the design spec + implementation plan (architecture, discovery rules, DB schema v1, key algorithms, packaging). Consult it before larger changes — but note it predates the code in places (e.g. §0 frames background work as "tokio (optional) or std threads"; the code uses std threads + mpsc).

Roadmap lives in two spots:
- `README.md` "Roadmap" — milestones M1–M6, all shipped.
- `docs/spec_and_plan.md` §14 "Future Enhancements" — the unbuilt backlog: GitHub issues/PR summary, project templates + init, file watcher for incremental updates, plugin system (custom detectors). (Health checks shipped as M7; export report as M8.)

Health checks live in `detect_health` (`discover.rs`) → `HealthChecks` on `Project` (`models.rs`) → `has_readme`/`has_license`/`has_tests`/`has_ci` columns (`db.rs`) → badges in the details panel (`health_badge` in `app.rs`). Detection is shallow (project root only).

Auto task tracking (`checklist.rs`, the only module with unit tests): `parse_checklist` reads GitHub-style `- [ ]`/`- [x]` items, **section-aware** — `##` headings group items into `ChecklistSection`s, and a section whose heading contains a backlog keyword (`backlog`/`later`/`someday`/`ideas`/`future`/`icebox`/`wishlist`) is parsed/shown but **excluded from the headline `total()`/`done()`**. The canonical source is a project-root **`ROADMAP.md`**; fallback order is `ROADMAP.md` → `ROADMAP` → `TODO.md` → `TODO` → `README.md` (first with checkboxes wins). Only that one file is parsed — `docs/` design docs are deliberately ignored to avoid counting non-task checkboxes. The convention is specified in `docs/roadmap-format.md`, and ProjectShelf's own `ROADMAP.md` is the reference example. It is **read-only** (the file is the source of truth; never written back). `build_project` caches headline counts in `TaskProgress` on `Project` (`task_total`/`task_done`/`task_source` columns); the full sectioned list is re-parsed on demand and cached in `app.rs`'s `task_cache` (invalidated on scan/watch). UI: a `☑ done/total` badge in list rows + a dedicated **Roadmap tab** in the details panel (`render_roadmap`) — progress bar (done/total + %) above an independently-scrollable item list (sections as headings, done items struck-through, backlog labeled). This replaced the old manual milestones feature.

The details panel (`render_project_details`) has a header (name/path, action buttons, Pin) above a tab bar driven by `DetailTab` (`detail_tab` field): **Overview** (`render_overview` — Activity + Health side-by-side via `ui.columns`, Languages, Export, a collapsed Tags editor), **Roadmap**, and **Notes**. The whole panel is wrapped in a `ScrollArea`.

Export report (`app.rs`): all exports flow through `build_export_row` + `write_export`, which render via `render_markdown`/`render_csv` and write into `config::data_dir()` (alongside the DB/settings). Two entry points: `export_report` (all projects → `projectshelf-report.{md,csv}`, buttons in the list toolbar) and `export_single` (selected project → `project-<sanitized-name>.{md,csv}`, buttons in the details panel). Tags are fetched per-project from the DB.

 
Below are two docs `spec.md` and `plan.md` as one file. I’m aiming for an MVP you can realistically ship, but with a clear path to “pro tool” features.

---

# spec.md — ProjectShelf - Project Manager Desktop App (Rust)

## 1. Summary

A local-first desktop application that indexes projects inside `~/Projects` and provides a fast UI to browse, search, sort, and inspect them. A “project” is typically a git repo, but the system supports explicit non-git projects via a marker file. The app surfaces activity signals (last commit, last file touch, dirty state, optional GitHub push time), detects primary languages used in each repo, and tracks per-project milestones and notes.

## 2. Goals

* Instantly show a list of projects under `~/Projects`.
* Provide fast search, filter, and sort (most recent activity, most stale, etc.).
* Show project details: path, git status, activity metrics, languages, milestones, notes.
* Cache results locally to avoid repeated expensive scans.
* Work well on Ubuntu (your primary environment) and be portable later.

## 3. Non-goals (initially)

* Acting as a full git client (merges, rebases, conflict resolution).
* Deep GitHub issue/PR integration (nice later).
* Cloud sync / multi-machine sync (later).

## 4. Target Platforms

* Primary: Linux (Ubuntu/Kubuntu).
* Secondary (later): Windows/macOS.

## 5. User Personas / Use Cases

* You have 40+ repos and want to quickly answer:

  * “What have I been working on recently?”
  * “Which projects are stale?”
  * “Open this repo in IDE/terminal quickly.”
  * “Where are my milestones and notes for this repo?”
  * “What language is this repo mostly written in?”

## 6. Definitions

* **Project Root**: A directory under `~/Projects` that is a project.
* **Git Project**: Project root containing `.git/` or a gitfile (submodule/worktree patterns).
* **Marked Project**: Project root containing `.projman/project.yaml` to force inclusion.

## 7. High-Level Architecture

* **Indexer/Scanner** (background):

  * Discovers projects, extracts metadata, updates cache.
* **Cache DB**:

  * SQLite database storing computed metadata and user-managed data (tags, pinned, local notes).
* **UI**:

  * Reads from cache; triggers background refresh; renders list and details.

## 8. Project Discovery Rules

Default root: `~/Projects` (configurable later).

Discovery:

1. Treat a directory as a project if:

   * It contains `.git` directory OR
   * It contains marker file `.projman/project.yaml`
2. Avoid scanning inside:

   * `node_modules`, `target`, `dist`, `build`, `.venv`, `.tox`, `.git`, `.next`, `.cache`, `out`
3. Max depth configurable (default: 4) to avoid runaway scans.

Edge cases:

* **Monorepos**: A monorepo root is a project. Nested repos are treated as separate projects only if they have `.git` OR explicit marker file, and are not ignored by `.projman/config`.
* **Worktrees**: detect `.git` file pointing elsewhere.

## 9. Metadata Captured per Project

### 9.1 Identity

* `project_id` (stable hash of canonical path)
* `name` (folder name default; overridden by metadata file)
* `path`
* `icon_kind` (derived from repo type)

### 9.2 Activity Signals (Local)

* `last_seen` (last scan time)
* `last_commit_ts` (from git log -1, if git)
* `last_fs_activity_ts` (max mtime in tree, excluding ignored dirs)
* `dirty` (uncommitted changes)
* `current_branch`
* `commit_count` (optional, expensive; later)

### 9.3 Remote Signals (Optional)

* `remote_url` (origin)
* `ahead_count`, `behind_count` (requires fetch or existing refs)
* `last_remote_update_ts` (timestamp of upstream tip commit)
* `github_pushed_at_ts` (from GitHub API, if configured)

### 9.4 Language Detection

Store:

* `language_breakdown`: list of `{language, bytes}` and computed percentages
* `primary_language`
* `top_languages` (top N)

Approach:

* Prefer **GitHub Linguist**-compatible detection offline (see Implementation section).
* Fallback heuristic by file extensions with ignore rules.

### 9.5 User Data

* `tags` (multi)
* `pinned` (bool)
* `notes` (markdown)
* `milestones` (list)

  * `id`, `title`, `status` (`todo|in_progress|blocked|done`), `due_date?`, `link?`

## 10. UI Requirements

### 10.1 Main Window Layout

* Left panel: project list

  * search box (fuzzy match)
  * filter chips (tags, language, dirty, stale, pinned)
  * sort dropdown:

    * most recent activity (max of last_commit and last_fs_activity)
    * most recent commit
    * most stale
    * most recently pushed (if available)
    * alphabetical
* Right panel: details

  * header: icon, name, path
  * quick actions:

    * Open folder
    * Open terminal here
    * Open in IDE (configurable command)
    * Open remote (GitHub) if present
  * sections:

    * Activity (timestamps, dirty, branch, ahead/behind)
    * Languages (top languages with percentages)
    * Milestones (editable list)
    * Notes (editable)

### 10.2 Icons

* Default icon per detected project type:

  * Rust (`Cargo.toml`)
  * Python (`pyproject.toml`, `requirements.txt`)
  * Node (`package.json`)
  * C++ (`CMakeLists.txt`)
  * Go (`go.mod`)
  * General git
* Use embedded SVG icons or a small bundled set.

### 10.3 Performance Targets

* Cold start (with cache): list visible in < 500ms.
* Background scan should not block UI.
* Full scan for ~40 repos should complete within a few seconds; network fetch/API optional.

## 11. Config

Config file (later): `~/.config/projman/config.toml`

* projects_root
* ignored_dirs
* max_depth
* ide_open_command
* enable_github_api
* github_token (or env var)
* fetch_remotes (bool) and interval

## 12. Data Storage

SQLite DB in: `~/.local/share/projman/projman.sqlite`

Tables (logical):

* `projects` (id, name, path, icon_kind, timestamps, git fields, language summary)
* `languages` (project_id, language, bytes)
* `user_meta` (project_id, pinned, notes)
* `tags` (project_id, tag)
* `milestones` (project_id, milestone fields)
* `scan_state` (last_scan_ts, version)

## 13. Security / Privacy

* Local-only by default.
* GitHub token stored in OS keyring if possible; otherwise env var recommended.
* No telemetry.

## 14. Future Enhancements

* GitHub issues/PR summary
* ~~“Health checks” (README/license/tests/CI)~~ — done
* Project templates + init
* ~~Export report (markdown/csv)~~ — done
* ~~File watcher for incremental updates~~ — done
* Plugin system (custom detectors)

---

# plan.md — Implementation Plan (Rust)

## 0. Recommended Rust Stack

* **UI**: `egui` via `eframe` (fast to build, cross-platform, great for dashboards)
* **DB**: `rusqlite`
* **Async / background work**: `tokio` (optional) or std threads + channels (simpler with egui)
* **Git**:

  * MVP: shell out to `git` CLI (most reliable)
  * Later: `gix` (Gitoxide) for pure-Rust reading
* **Language detection**:

  * MVP: extension-based + size counts + ignore rules
  * Upgrade: integrate `github-linguist` via subprocess (Ruby) *or* adopt a Rust linguist port if you choose one later

This hits your “use Rust” goal while keeping delivery realistic.

---

## 1. Milestone Breakdown

### Milestone 1 — Repo Discovery + Cached List (MVP core)

**Goal**: scan `~/Projects`, find projects, store basic rows, show list UI.

Tasks:

1. Create Rust workspace:

   * `projman_core` (scanner, models)
   * `projman_ui` (eframe UI)
2. Implement discovery:

   * DFS walk with ignore dirs + max depth
   * identify `.git` or `.projman/project.yaml`
3. SQLite schema v1:

   * `projects(project_id TEXT PK, name TEXT, path TEXT, last_seen INTEGER, icon_kind TEXT)`
4. UI:

   * left list with search
   * right panel shows basic details (name/path)
5. Background scan:

   * On startup: load cached list immediately
   * Spawn scanner thread; push updates to UI via channel; UI writes to DB

Acceptance:

* App launches and displays all projects from `~/Projects`.
* Refresh button rescans.

---

### Milestone 2 — Git Metadata + Activity Sorting

**Goal**: show last commit, dirty status, branch; sort by activity.

Tasks:

1. Git CLI calls (per project):

   * `git -C <path> rev-parse --is-inside-work-tree`
   * `git -C <path> log -1 --format=%ct`
   * `git -C <path> status --porcelain`
   * `git -C <path> rev-parse --abbrev-ref HEAD`
2. Filesystem last activity:

   * walk tree excluding ignored dirs; compute max `mtime`
3. DB fields:

   * last_commit_ts, last_fs_activity_ts, dirty, branch
4. UI:

   * sort dropdown (recent activity, stale, alpha)
   * badges: dirty, branch

Acceptance:

* You can sort by “recent activity” and see stale projects.
* Clicking a project shows git details.

---

### Milestone 3 — Language Detection (Requested)

**Goal**: compute top languages and show them.

MVP approach (fast + good enough):

* Map extensions → language (e.g. `.rs` Rust, `.py` Python, `.ts` TypeScript, `.cpp` C++, etc.)
* Count total bytes per language by summing file sizes.
* Ignore:

  * `.git`, build outputs, vendor dirs, lockfiles optionally
* Store top N languages.

Tasks:

1. Implement extension mapping table + ignore list.
2. Scanner computes `HashMap<Lang, bytes>`.
3. DB:

   * `languages(project_id, language, bytes)`
   * `projects.primary_language`
4. UI:

   * show top 3 languages + percentage bar (simple horizontal bar)

Acceptance:

* Each project shows primary language and top 3 breakdown.

Upgrade path (optional later):

* Add a “Linguist mode” toggle:

  * If `github-linguist` is installed, call it for better accuracy.
  * Otherwise use heuristic.

---

### Milestone 4 — Milestones + Notes System

**Goal**: track your milestones in a consistent way and edit them in-app.

Decision:

* Store user-managed milestones/notes in the DB *and* optionally sync with a per-repo file:

  * `.projman/project.yaml`
    Why:
* DB gives fast search and global view.
* File keeps it portable with the repo and editable outside app.

Tasks:

1. Define YAML format + parser (`serde` + `serde_yaml`).
2. On scan:

   * if `.projman/project.yaml` exists, import/merge into DB
3. UI editor:

   * milestones list with status dropdown
   * notes markdown editor (plain text is fine MVP)
4. Export:

   * “Write back” button to update `.projman/project.yaml`

Acceptance:

* You can track milestones per project and see them in details.
* Notes persist.

---

### Milestone 5 — GitHub / Remote “Last Pushed” (Optional)

**Goal**: show “last pushed to GitHub” reliably.

Tasks:

1. Detect `origin` URL and parse GitHub owner/repo.
2. If token configured:

   * call GitHub API `repos/{owner}/{repo}` and store `pushed_at`
3. Fallback:

   * `git fetch` (optional) then get upstream tip commit timestamp.
4. UI:

   * show “GitHub pushed” if available; otherwise “Remote tip updated”.

Acceptance:

* Projects show push-ish recency when configured.

---

## 2. File/Module Structure (Suggested)

```
projman/
  Cargo.toml (workspace)
  crates/
    projman_core/
      src/
        lib.rs
        config.rs
        discover.rs
        scan.rs
        git.rs
        languages.rs
        db.rs
        models.rs
        projfile.rs
    projman_ui/
      src/
        main.rs
        app.rs
        ui_list.rs
        ui_details.rs
        actions.rs
```

---

## 3. DB Schema v1 (Practical)

* `projects`

  * `project_id TEXT PRIMARY KEY`
  * `name TEXT`
  * `path TEXT UNIQUE`
  * `icon_kind TEXT`
  * `last_seen INTEGER`
  * `last_commit_ts INTEGER`
  * `last_fs_activity_ts INTEGER`
  * `dirty INTEGER`
  * `branch TEXT`
  * `primary_language TEXT`
* `languages`

  * `project_id TEXT`
  * `language TEXT`
  * `bytes INTEGER`
* `user_meta`

  * `project_id TEXT`
  * `pinned INTEGER`
  * `notes TEXT`
* `milestones`

  * `project_id TEXT`
  * `id TEXT`
  * `title TEXT`
  * `status TEXT`
  * `due_ts INTEGER NULL`
  * `link TEXT NULL`
* `tags`

  * `project_id TEXT`
  * `tag TEXT`
* `scan_state`

  * `last_scan_ts INTEGER`
  * `version INTEGER`

---

## 4. Key Algorithms / Notes

### 4.1 Stable Project IDs

`project_id = sha256(canonical_path)` (store as hex).
Keeps IDs stable even if name changes.

### 4.2 Activity Score

`activity_ts = max(last_commit_ts, last_fs_activity_ts)`
Sort by that for “most active”.

### 4.3 Scanning Strategy

* Start with cache → render
* Spawn scan worker:

  * discovery pass
  * per-project metadata pass (parallel with a small thread pool)
* Send incremental updates to UI

---

## 5. Packaging (Linux)

* MVP: `cargo build --release`
* Later:

  * AppImage (common for Linux distribution)
  * `.deb` if you want system install

---

## 6. Acceptance Checklist for “Useful MVP”

* [ ] Shows all projects in `~/Projects`
* [ ] Search + sort by recent activity + stale
* [ ] Shows git status + last commit
* [ ] Shows language breakdown
* [ ] Notes + milestones per project
* [ ] One-click open folder + open terminal + open in IDE


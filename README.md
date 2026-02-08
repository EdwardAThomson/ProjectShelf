# ProjectShelf

A local-first desktop application that indexes projects inside `~/Projects` and provides a fast UI to browse, search, sort, and inspect them.

![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/License-MIT-blue.svg)

## Features

- **Fast Project Discovery** — Automatically scans `~/Projects` for git repos and marked projects
- **Git Integration** — Shows branch, dirty status, and last commit time
- **Activity Tracking** — Tracks filesystem activity and sorts by most recent/stale
- **Smart Icons** — Detects project type (Rust, Python, Node, Go, C++) from manifest files
- **Quick Actions** — Open folder, terminal, or IDE with one click
- **Search & Sort** — Fuzzy search and multiple sort modes
- **Local Cache** — SQLite database for instant startup

## Screenshots

*Coming soon*

## Requirements

- **Rust** 1.75+ (2024 edition)
- **Git** (for git metadata extraction)
- **Linux** (Ubuntu/Kubuntu primary; other distros should work)

### Linux Dependencies

On Ubuntu/Debian, you may need:

```bash
sudo apt install libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
```

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/EdwardAThomson/ProjectShelf.git
cd ProjectShelf

# Build release binary
cargo build --release

# Run the application
./target/release/projectshelf
```

### Development Build

```bash
# Quick development build
cargo run -p projectshelf_ui --bin projectshelf
```

## Usage

1. **Launch the app** — It will automatically scan `~/Projects` on startup
2. **Browse projects** — Use the left panel to see all discovered projects
3. **Search** — Type in the search box to filter by name or path
4. **Sort** — Use the dropdown to sort by:
   - Recent Activity (default)
   - Most Stale
   - Alphabetical
5. **View details** — Click a project to see git status, timestamps, and more
6. **Quick actions** — Use buttons to open folder, terminal, or VS Code

### Project Detection

A directory is recognized as a project if it contains:
- `.git/` directory (git repository)
- `.projman/project.yaml` (explicit marker file)

### Ignored Directories

The scanner skips these directories for performance:
`node_modules`, `target`, `dist`, `build`, `.venv`, `.tox`, `.git`, `.next`, `.cache`, `out`, `vendor`, `__pycache__`

## Project Structure

```
ProjectShelf/
├── Cargo.toml              # Workspace manifest
├── crates/
│   ├── projman_core/       # Core library
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs   # Configuration constants
│   │       ├── db.rs       # SQLite database
│   │       ├── discover.rs # Project discovery
│   │       ├── fs_activity.rs # Filesystem mtime scanning
│   │       ├── git.rs      # Git CLI integration
│   │       └── models.rs   # Data structures
│   └── projman_ui/         # Desktop UI
│       └── src/
│           ├── main.rs
│           └── app.rs      # egui application
└── spec_and_plan.md        # Design specification
```

## Data Storage

- **Database**: `~/.local/share/projman/projman.sqlite`
- **Config** (future): `~/.config/projman/config.toml`

## Roadmap

- [x] **M1** — Project discovery + cached list
- [x] **M2** — Git metadata + activity sorting
- [x] **M3** — Language detection with byte counts
- [x] **M4** — Milestones + notes per project
- [x] **M5** — GitHub remote integration

## Tech Stack

- **UI**: [egui](https://github.com/emilk/egui) via eframe
- **Database**: [rusqlite](https://github.com/rusqlite/rusqlite) (SQLite)
- **Git**: CLI subprocess calls (reliable across systems)

## License

MIT
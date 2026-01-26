use std::path::PathBuf;

pub const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    ".tox",
    ".git",
    ".next",
    ".cache",
    "out",
    "vendor",
    "__pycache__",
];

pub const MAX_DEPTH: usize = 4;

pub fn projects_root() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join("Projects")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("Could not find local data directory")
        .join("projman")
}

pub fn db_path() -> PathBuf {
    data_dir().join("projman.sqlite")
}

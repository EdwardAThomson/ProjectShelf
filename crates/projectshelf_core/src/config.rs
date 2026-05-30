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
    // CMake nests all build artifacts (incl. `*.o.d` dependency files that would
    // otherwise be miscounted as the D language) under CMakeFiles/, regardless of
    // the build dir's name.
    "CMakeFiles",
    "cmake-build-debug",
    "cmake-build-release",
    // Vendored / archived third-party code — not the project's own, and easily
    // skews language stats (e.g. a bundled C crypto lib outweighing the app's C++).
    "third_party",
    "third-party",
    "external",
    "extern",
    "deps",
    "subprojects",
    "archive",
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

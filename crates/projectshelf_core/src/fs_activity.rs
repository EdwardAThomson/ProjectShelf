use crate::config::IGNORED_DIRS;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub fn get_last_fs_activity(path: &Path) -> Option<i64> {
    let mut max_mtime: Option<i64> = None;
    scan_for_mtime(path, 0, &mut max_mtime);
    max_mtime
}

fn scan_for_mtime(path: &Path, depth: usize, max_mtime: &mut Option<i64>) {
    if depth > 5 {
        return;
    }

    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if IGNORED_DIRS.contains(&dir_name) {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if IGNORED_DIRS.contains(&name) {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                    let ts = duration.as_secs() as i64;
                    *max_mtime = Some(max_mtime.map_or(ts, |m| m.max(ts)));
                }
            }

            if metadata.is_dir() {
                scan_for_mtime(&entry_path, depth + 1, max_mtime);
            }
        }
    }
}

//! Ad-hoc audit: for each discovered project, list EVERY markdown file (root +
//! one level into common doc dirs) that contains task checkboxes, with counts.
//! Surfaces projects with multiple/competing roadmap files vs. what the current
//! single-file `parse_checklist` would pick.
//!   cargo run -p projectshelf_core --example checktasks [ROOT]

use projectshelf_core::{config, parse_checklist, scan_projects};
use std::fs;
use std::path::{Path, PathBuf};

fn count_checkboxes(file: &Path) -> Option<(u32, u32)> {
    let content = fs::read_to_string(file).ok()?;
    let mut total = 0;
    let mut done = 0;
    for line in content.lines() {
        let t = line.trim_start();
        let after = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("+ "));
        if let Some(rest) = after {
            let inner = rest.trim_start();
            if inner.starts_with("[ ]") {
                total += 1;
            } else if inner.starts_with("[x]") || inner.starts_with("[X]") {
                total += 1;
                done += 1;
            }
        }
    }
    if total > 0 { Some((total, done)) } else { None }
}

/// Markdown files worth checking: project root + one level into doc dirs.
fn candidate_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut scan = |d: &Path| {
        if let Ok(entries) = fs::read_dir(d) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("md")
                    || p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| {
                            let l = n.to_lowercase();
                            l == "todo" || l == "roadmap"
                        })
                        .unwrap_or(false)
                {
                    out.push(p);
                }
            }
        }
    };
    scan(dir);
    for sub in ["docs", "doc", ".github"] {
        scan(&dir.join(sub));
    }
    out
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(config::projects_root);

    println!("Scanning {} ...\n", root.display());
    let projects = scan_projects(&root);

    let mut multi = 0;
    for (p, _l) in &projects {
        let dir = Path::new(&p.path);
        let mut found: Vec<(String, u32, u32)> = Vec::new();
        for f in candidate_files(dir) {
            if let Some((total, done)) = count_checkboxes(&f) {
                let rel = f.strip_prefix(dir).unwrap_or(&f).display().to_string();
                found.push((rel, total, done));
            }
        }
        if found.is_empty() {
            continue;
        }

        let picked = parse_checklist(dir).map(|c| c.source).unwrap_or_default();
        let flag = if found.len() > 1 { " ⚠ MULTIPLE" } else { "" };
        if found.len() > 1 {
            multi += 1;
        }
        println!("{}{}", p.name, flag);
        println!("   @ {}", p.path);
        for (rel, total, done) in &found {
            let mark = if *rel == picked { "→ PICKED" } else { "        " };
            println!("   {} {:<28} {}/{}", mark, rel, done, total);
        }
        println!();
    }

    println!(
        "{} projects scanned; {} have a checklist file; {} have MORE THAN ONE.",
        projects.len(),
        projects.iter().filter(|(p, _)| p.tasks.total > 0).count(),
        multi
    );
}

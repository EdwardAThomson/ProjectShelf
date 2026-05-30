use std::fs;
use std::path::Path;

/// A single markdown task-list item (`- [ ]` / `- [x]`).
#[derive(Debug, Clone)]
pub struct ChecklistItem {
    pub done: bool,
    pub text: String,
}

/// A group of task items under one `##` heading (a milestone/phase). Items
/// before any heading land in a section with `title == None`.
#[derive(Debug, Clone)]
pub struct ChecklistSection {
    pub title: Option<String>,
    /// Whether this section counts toward the headline progress %. Backlog-type
    /// sections (see [`is_backlog`]) are shown but not counted.
    pub counted: bool,
    pub items: Vec<ChecklistItem>,
}

/// Parsed task list for a project, sourced from a tracked markdown file
/// (canonically `ROADMAP.md`).
#[derive(Debug, Clone)]
pub struct Checklist {
    /// The filename the items came from (e.g. `ROADMAP.md`).
    pub source: String,
    pub sections: Vec<ChecklistSection>,
}

impl Checklist {
    /// All items across every section, regardless of counted/backlog status.
    pub fn items(&self) -> impl Iterator<Item = &ChecklistItem> {
        self.sections.iter().flat_map(|s| s.items.iter())
    }

    /// Headline total — counted (non-backlog) sections only.
    pub fn total(&self) -> u32 {
        self.counted_items().count() as u32
    }

    /// Headline done — counted (non-backlog) sections only.
    pub fn done(&self) -> u32 {
        self.counted_items().filter(|i| i.done).count() as u32
    }

    fn counted_items(&self) -> impl Iterator<Item = &ChecklistItem> {
        self.sections
            .iter()
            .filter(|s| s.counted)
            .flat_map(|s| s.items.iter())
    }
}

/// Candidate files in priority order. Canonical is `ROADMAP.md`; the rest are
/// fallbacks so untidied projects still show progress during migration. The
/// first file that exists *and* contains at least one checkbox wins.
const CANDIDATES: &[&str] = &["roadmap.md", "roadmap", "todo.md", "todo", "readme.md", "readme"];

/// Heading keywords that mark a section as backlog (shown but not counted).
const BACKLOG_KEYWORDS: &[&str] = &[
    "backlog", "later", "someday", "ideas", "future", "icebox", "wishlist",
];

/// Parse the canonical roadmap (or a fallback) for a project. Returns `None`
/// if no checkboxes are found in any candidate file.
pub fn parse_checklist(project_path: &Path) -> Option<Checklist> {
    // Map lowercased candidate name -> actual on-disk filename (one dir read).
    let entries = fs::read_dir(project_path).ok()?;
    let mut present: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        present.push((name.to_lowercase(), name));
    }

    for candidate in CANDIDATES {
        let Some((_, actual)) = present.iter().find(|(lower, _)| lower == candidate) else {
            continue;
        };
        let content = match fs::read_to_string(project_path.join(actual)) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let sections = parse_sections(&content);
        if sections.iter().any(|s| !s.items.is_empty()) {
            return Some(Checklist {
                source: actual.clone(),
                sections,
            });
        }
    }
    None
}

/// Split markdown into sections by heading, attributing checkboxes to the
/// nearest heading above them. Empty (item-less) sections are dropped.
fn parse_sections(content: &str) -> Vec<ChecklistSection> {
    let mut sections: Vec<ChecklistSection> = Vec::new();
    let mut current = ChecklistSection {
        title: None,
        counted: true,
        items: Vec::new(),
    };

    for line in content.lines() {
        if let Some(title) = heading_title(line) {
            if !current.items.is_empty() {
                sections.push(current);
            }
            let counted = !is_backlog(&title);
            current = ChecklistSection {
                title: Some(title),
                counted,
                items: Vec::new(),
            };
        } else if let Some(item) = parse_checkbox(line) {
            current.items.push(item);
        }
    }
    if !current.items.is_empty() {
        sections.push(current);
    }
    sections
}

/// Return a markdown heading's text (any level), or `None` if not a heading.
fn heading_title(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let title = trimmed.trim_start_matches('#').trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// Whether a heading marks a backlog-type (uncounted) section.
fn is_backlog(title: &str) -> bool {
    let lower = title.to_lowercase();
    BACKLOG_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Parse a single line as a GitHub-style task item. Recognizes `[ ]` (todo)
/// and `[x]`/`[X]` (done) after a `-`, `*`, or `+` list marker, at any
/// indentation. Anything else returns `None`.
fn parse_checkbox(line: &str) -> Option<ChecklistItem> {
    let trimmed = line.trim_start();
    let after_marker = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;

    let inner = after_marker.trim_start().strip_prefix('[')?;
    let mut chars = inner.chars();
    let mark = chars.next()?;
    let text = chars.as_str().strip_prefix(']')?;

    let done = match mark {
        'x' | 'X' => true,
        ' ' => false,
        _ => return None,
    };

    Some(ChecklistItem {
        done,
        text: text.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Option<(bool, String)> {
        parse_checkbox(line).map(|i| (i.done, i.text))
    }

    #[test]
    fn recognizes_checkboxes() {
        assert_eq!(parsed("- [ ] todo item"), Some((false, "todo item".into())));
        assert_eq!(parsed("- [x] done item"), Some((true, "done item".into())));
        assert_eq!(parsed("- [X] upper done"), Some((true, "upper done".into())));
        assert_eq!(parsed("  - [ ] indented"), Some((false, "indented".into())));
        assert_eq!(parsed("* [x] star marker"), Some((true, "star marker".into())));
        assert_eq!(parsed("+ [ ] plus marker"), Some((false, "plus marker".into())));
    }

    #[test]
    fn rejects_non_checkboxes() {
        assert_eq!(parsed("- regular bullet"), None);
        assert_eq!(parsed("## Heading"), None);
        assert_eq!(parsed("plain text"), None);
        assert_eq!(parsed("- [-] non-standard mark"), None);
        assert_eq!(parsed("[x] no list marker"), None);
    }

    /// Create a fresh, empty temp dir unique to `tag` for an end-to-end test.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("projectshelf_checklist_test_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_roadmap_md_with_bold_milestones() {
        let dir = temp_dir("roadmap_md");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap\n\n- [x] **M1** — Discovery\n- [x] **M2** — Git metadata\n- [ ] **M3** — Templates\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("ROADMAP.md should be parsed");
        assert_eq!(cl.source, "ROADMAP.md");
        assert_eq!(cl.total(), 3);
        assert_eq!(cl.done(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn groups_by_section_and_excludes_backlog() {
        let dir = temp_dir("sections");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap — Demo\n\n## Now\n- [x] core\n- [ ] settings\n\n## Next\n- [ ] export\n\n## Backlog\n- [ ] plugins\n- [ ] themes\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("should parse");
        // Three sections: Now, Next, Backlog.
        assert_eq!(cl.sections.len(), 3);
        assert_eq!(cl.sections[2].title.as_deref(), Some("Backlog"));
        assert!(!cl.sections[2].counted);

        // Headline counts Now + Next only (3 items, 1 done); backlog excluded.
        assert_eq!(cl.total(), 3);
        assert_eq!(cl.done(), 1);
        // All items including backlog = 5.
        assert_eq!(cl.items().count(), 5);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_roadmap_section_inside_readme() {
        let dir = temp_dir("readme_roadmap");
        std::fs::write(
            dir.join("README.md"),
            "# Project\n\nSome prose.\n\n## Roadmap\n\n- [x] M1 done\n- [ ] M2 todo\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("README roadmap section should be parsed");
        assert_eq!(cl.source, "README.md");
        assert_eq!(cl.total(), 2);
        assert_eq!(cl.done(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roadmap_takes_priority_over_todo() {
        let dir = temp_dir("priority");
        std::fs::write(dir.join("ROADMAP.md"), "- [x] canonical task\n").unwrap();
        std::fs::write(dir.join("TODO.md"), "- [ ] a\n- [ ] b\n").unwrap();

        let cl = parse_checklist(&dir).expect("a checklist should be found");
        assert_eq!(cl.source, "ROADMAP.md");
        assert_eq!(cl.total(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_candidate_without_checkboxes() {
        let dir = temp_dir("skip_empty");
        std::fs::write(dir.join("ROADMAP.md"), "# Roadmap\n\nJust prose, no boxes.\n").unwrap();
        std::fs::write(dir.join("README.md"), "- [x] real task\n").unwrap();

        let cl = parse_checklist(&dir).expect("should fall through to README.md");
        assert_eq!(cl.source, "README.md");
        assert_eq!(cl.total(), 1);
        assert_eq!(cl.done(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_checklist_files_returns_none() {
        let dir = temp_dir("none");
        std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
        assert!(parse_checklist(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

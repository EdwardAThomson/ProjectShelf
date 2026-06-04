use std::fs;
use std::path::Path;

/// A single markdown task-list item (`- [ ]` / `- [x]`).
#[derive(Debug, Clone)]
pub struct ChecklistItem {
    pub done: bool,
    pub text: String,
    /// Optional clarifying prose written on the line(s) directly beneath the
    /// item (indented continuation, no checkbox). Lets a terse item carry a
    /// fuller explanation without bloating the headline text.
    pub detail: Option<String>,
}

/// A group of task items under one `##` heading (a milestone/phase). Items
/// before any heading land in a section with `title == None`.
///
/// Sections are kept **flat** but carry their heading `level` so the UI can
/// render hierarchy: a deeper section (e.g. `###`) is a sub-phase of the most
/// recent shallower one (e.g. `##`). A heading with no items of its own is kept
/// when it has a description, so an umbrella phase keeps its intro and groups
/// its sub-phases.
#[derive(Debug, Clone, Default)]
pub struct ChecklistSection {
    pub title: Option<String>,
    /// Markdown heading depth: 2 for `##`, 3 for `###`, etc. The implicit
    /// pre-heading section uses 2.
    pub level: usize,
    /// Prose written under the heading, before its first item — a one-line
    /// description of what the section is about.
    pub description: Option<String>,
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
    /// The file preamble — prose between the title and the first `##` section
    /// (the `_Status_` metadata line excluded). Used as the project description.
    pub description: Option<String>,
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
        let (description, sections) = parse_sections(&content);
        if sections.iter().any(|s| !s.items.is_empty()) {
            return Some(Checklist {
                source: actual.clone(),
                description,
                sections,
            });
        }
    }
    None
}

/// Split markdown into sections by heading, attributing checkboxes to the
/// nearest heading above them, and capturing the surrounding prose:
/// - the **file description** (preamble before the first `##` section),
/// - each section's **description** (prose between its heading and first item),
/// - each item's **detail** (indented continuation prose directly below it).
///
/// The level-1 `#` title and the `_Status_` metadata line are skipped. Empty
/// (item-less) sections are dropped. Returns `(file_description, sections)`.
fn parse_sections(content: &str) -> (Option<String>, Vec<ChecklistSection>) {
    let mut file_desc: Option<String> = None;
    let mut sections: Vec<ChecklistSection> = Vec::new();
    let mut current = ChecklistSection {
        title: None,
        level: 2,
        description: None,
        counted: true,
        items: Vec::new(),
    };
    // Pending prose lines for the current section's description (before items).
    let mut prose: Vec<String> = Vec::new();
    // Index of the last item, for attaching indented detail lines below it.
    let mut last_item: Option<usize> = None;
    // Stack of (heading level, counted) ancestors, so a section nested under a
    // backlog heading inherits its uncounted status.
    let mut stack: Vec<(usize, bool)> = Vec::new();

    // Flush pending prose into the file description (if we're still above the
    // first section) or the current section's description.
    macro_rules! flush_prose {
        () => {
            if !prose.is_empty() {
                let joined = prose.join(" ");
                if current.title.is_none() && sections.is_empty() {
                    file_desc.get_or_insert(joined);
                } else if current.description.is_none() {
                    current.description = Some(joined);
                }
                prose.clear();
            }
        };
    }

    for line in content.lines() {
        if is_status_line(line) {
            continue;
        }

        if let Some((level, title)) = heading(line) {
            flush_prose!();
            last_item = None;
            // The level-1 title isn't a section; keep accumulating beneath it.
            if level == 1 {
                continue;
            }
            // Close out the current section if it carries items, or is a
            // header-only umbrella with an intro description to preserve.
            if !current.items.is_empty() || current.description.is_some() {
                sections.push(std::mem::take(&mut current));
            }
            // A section is uncounted if it's backlog-typed, or nested under one.
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            let parent_counted = stack.last().map(|(_, c)| *c).unwrap_or(true);
            let counted = parent_counted && !is_backlog(&title);
            stack.push((level, counted));
            current.title = Some(title.clone());
            current.level = level;
            current.counted = counted;
            continue;
        }

        if let Some(item) = parse_checkbox(line) {
            flush_prose!();
            current.items.push(item);
            last_item = Some(current.items.len() - 1);
            continue;
        }

        let text = clean_prose(line);
        if text.is_empty() {
            // A blank line ends an item's detail block.
            last_item = None;
            continue;
        }
        match last_item {
            Some(idx) => match &mut current.items[idx].detail {
                Some(d) => {
                    d.push(' ');
                    d.push_str(&text);
                }
                None => current.items[idx].detail = Some(text),
            },
            None => prose.push(text),
        }
    }

    flush_prose!();
    if !current.items.is_empty() || current.description.is_some() {
        sections.push(current);
    }
    (file_desc, sections)
}

/// Return a markdown heading's level (number of leading `#`) and text, or
/// `None` if the line isn't a heading.
fn heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    let title = trimmed[level..].trim();
    if title.is_empty() {
        None
    } else {
        Some((level, title.to_string()))
    }
}

/// Whether a line is the `_Status: ... _` metadata line (excluded from the
/// description).
fn is_status_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('_') && t.to_lowercase().contains("status")
}

/// Trim a prose line, stripping any leading markdown blockquote markers (`>`)
/// so blockquote-style preambles read as plain text.
fn clean_prose(line: &str) -> String {
    let mut s = line.trim();
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    s.to_string()
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
        detail: None,
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

    #[test]
    fn captures_preamble_as_description() {
        let dir = temp_dir("preamble");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap — Demo\n\n_Status: active · updated 2026-06-03_\n\nA small tool that does a useful thing.\nSecond line of the intro.\n\n## Next\n- [ ] ship it\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("should parse");
        assert_eq!(
            cl.description.as_deref(),
            Some("A small tool that does a useful thing. Second line of the intro.")
        );
        // The _Status_ line must not leak into the description.
        assert!(!cl.description.as_deref().unwrap().contains("Status"));
        assert_eq!(cl.total(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strips_blockquote_markers_from_preamble() {
        let dir = temp_dir("blockquote");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap\n\n> A blockquote-style intro paragraph.\n\n## Now\n- [x] done\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("should parse");
        assert_eq!(
            cl.description.as_deref(),
            Some("A blockquote-style intro paragraph.")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn captures_section_description_and_item_detail() {
        let dir = temp_dir("detail");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap\n\n## Next\nWhat we're working on right now.\n- [ ] Cryptic item\n      A fuller explanation of what this actually means\n      spread over two lines.\n- [ ] Plain item\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("should parse");
        let next = cl
            .sections
            .iter()
            .find(|s| s.title.as_deref() == Some("Next"))
            .expect("Next section");
        assert_eq!(
            next.description.as_deref(),
            Some("What we're working on right now.")
        );
        assert_eq!(next.items.len(), 2);
        assert_eq!(
            next.items[0].detail.as_deref(),
            Some("A fuller explanation of what this actually means spread over two lines.")
        );
        assert_eq!(next.items[1].detail, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_umbrella_heading_and_records_levels() {
        let dir = temp_dir("hierarchy");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap\n\n## Phase 5 — Refactor\nThe big cleanup phase.\n\n### 5.1 — First sub\n- [x] did a thing\n\n### 5.2 — Second sub\n- [ ] do another thing\n\n## Phase 1 — Core\n- [x] shipped\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("should parse");
        // Umbrella (no items, has description) is kept, then its two sub-phases,
        // then the flat Phase 1 — four sections total.
        assert_eq!(cl.sections.len(), 4);

        let umbrella = &cl.sections[0];
        assert_eq!(umbrella.title.as_deref(), Some("Phase 5 — Refactor"));
        assert_eq!(umbrella.level, 2);
        assert!(umbrella.items.is_empty());
        assert_eq!(umbrella.description.as_deref(), Some("The big cleanup phase."));

        assert_eq!(cl.sections[1].title.as_deref(), Some("5.1 — First sub"));
        assert_eq!(cl.sections[1].level, 3);
        assert_eq!(cl.sections[2].level, 3);
        assert_eq!(cl.sections[3].title.as_deref(), Some("Phase 1 — Core"));
        assert_eq!(cl.sections[3].level, 2);

        // Header-only umbrella contributes no items to the headline count.
        assert_eq!(cl.total(), 3);
        assert_eq!(cl.done(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subsections_inherit_backlog_from_parent() {
        let dir = temp_dir("backlog_inherit");
        std::fs::write(
            dir.join("ROADMAP.md"),
            "# Roadmap\n\n## Now\n- [ ] real work\n\n## Backlog\n### Deployment\n- [ ] ship later\n- [ ] also later\n",
        )
        .unwrap();

        let cl = parse_checklist(&dir).expect("should parse");
        let deployment = cl
            .sections
            .iter()
            .find(|s| s.title.as_deref() == Some("Deployment"))
            .expect("Deployment section");
        // Nested under Backlog → not counted, even though its own title has no
        // backlog keyword.
        assert!(!deployment.counted);
        // Headline counts only the one item under Now.
        assert_eq!(cl.total(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

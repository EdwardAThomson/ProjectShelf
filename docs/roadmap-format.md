# Canonical `ROADMAP.md` format

ProjectShelf treats a project's roadmap as a **reflection of a file you already maintain**, not a separate tracker. To make that reliable across messy repos, every project should keep a single canonical roadmap file in one place and shape.

## Location

- **`ROADMAP.md` at the project root.** One file, conventional name.
- It is the **only** file parsed for task progress when present. Design docs, architecture notes, comparisons, etc. stay under `docs/` and are **ignored** — that is deliberate, so non-task checkboxes don't pollute progress.
- If a project has no `ROADMAP.md` yet, the parser falls back (in order) to `ROADMAP` → `TODO.md` → `TODO` → `README.md`, so untidied projects still show something during migration.

## Structure

Plain GitHub-flavored markdown, so it renders anywhere:

```markdown
# Roadmap — <Project Name>

_Status: active · updated YYYY-MM-DD_   <!-- optional, ignored by the parser -->

## <Milestone or phase>
- [x] A completed task
- [ ] An open task

## <Next milestone>
- [ ] Planned task

## Backlog
- [ ] Someday / maybe idea
```

Rules the parser follows:

- **`##` headings are milestones/phases.** Checkboxes belong to the nearest heading above them. This gives per-section progress, and is how scattered files get collated into one coherent roadmap.
- **Checkboxes are binary:** `- [ ]` (open) and `- [x]` / `- [X]` (done). `*` and `+` markers and indentation are allowed. Anything else (`- [-]`, plain bullets) is not a task.
- **Backlog-type sections are not counted** in the headline progress %, but are still shown. A section counts as backlog if its heading contains any of: `backlog`, `later`, `someday`, `ideas`, `future`, `icebox`, `wishlist`.
- The `#` title line and any prose/`_status_` lines are ignored.

## Headline progress

`done / total` shown in the app = the sum across **counted** sections only. Backlog items are tracked and displayed but excluded so a long someday-list doesn't make a project look perpetually unfinished.

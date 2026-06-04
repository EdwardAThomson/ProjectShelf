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

A one-paragraph description of what this project is. Shown as the project
description in the Overview tab — keep it to a sentence or two.

## <Milestone or phase>
One line on what this phase is about (optional).

- [x] A completed task
- [ ] An open task
      An optional indented line clarifying what a terse item actually means.

## <Next milestone>
- [ ] Planned task

## Backlog
- [ ] Someday / maybe idea
```

Rules the parser follows:

- **The preamble is the project description.** Prose between the `#` title and the first `##` section (excluding the `_Status_` line) is captured and shown as the project's description in the Overview tab. A leading `>` blockquote marker is stripped. Keep it short — a sentence or two.
- **`##` headings are milestones/phases.** Checkboxes belong to the nearest heading above them. This gives per-section progress, and is how scattered files get collated into one coherent roadmap.
- **`###` (and deeper) headings are sub-phases.** A deeper heading is treated as a child of the nearest shallower one and rendered indented beneath it in the Roadmap tab. Use this to break a large phase into numbered sub-phases without flattening them into siblings of the other top-level phases. An umbrella `##` phase with no checkboxes of its own is kept (with its intro description) as long as it has sub-phases or a description — so it still groups its children.
- **Backlog status is inherited.** A section nested under a backlog heading (e.g. `### Deployment` under `## Backlog`) is itself treated as backlog — not counted in the headline % — even if its own heading has no backlog keyword. Don't bake status into headings (`### Foo ✅ done`); the checkbox state is the source of truth.
- **A section may carry a one-line description.** Prose written under a `##` heading, before its first checkbox, is shown as muted text beneath the heading in the Roadmap tab.
- **An item may carry detail.** A non-checkbox line directly beneath an item (conventionally indented, no blank line between) is attached as that item's detail and shown as muted text under it in the Roadmap tab. Use this to explain a cryptic one-liner without bloating the headline text. A blank line ends the detail block.
- **Checkboxes are binary:** `- [ ]` (open) and `- [x]` / `- [X]` (done). `*` and `+` markers and indentation are allowed. Anything else (`- [-]`, plain bullets) is not a task.
- **Backlog-type sections are not counted** in the headline progress %, but are still shown. A section counts as backlog if its heading contains any of: `backlog`, `later`, `someday`, `ideas`, `future`, `icebox`, `wishlist`.
- The `#` title line and the `_Status_` line are ignored.

## Headline progress

`done / total` shown in the app = the sum across **counted** sections only. Backlog items are tracked and displayed but excluded so a long someday-list doesn't make a project look perpetually unfinished.

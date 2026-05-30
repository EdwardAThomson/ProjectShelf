use eframe::egui;
use notify::{RecursiveMode, Watcher};
use projectshelf_core::{
    config, discover_projects, find_project_root, parse_checklist, scan_projects,
    scan_single_project, Checklist, Database, LanguageBreakdown, Project, ProjectFile, UserMeta,
    IGNORED_DIRS,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

pub struct ProjectShelfApp {
    projects: Vec<Project>,
    languages: HashMap<String, LanguageBreakdown>,
    selected_idx: Option<usize>,
    search_query: String,
    db: Option<Database>,
    scan_rx: Option<Receiver<Vec<(Project, LanguageBreakdown)>>>,
    scan_tx: Option<Sender<()>>,
    is_scanning: bool,
    sort_mode: SortMode,
    detail_tab: DetailTab,
    user_meta: HashMap<String, UserMeta>,
    tags: HashMap<String, Vec<String>>,
    all_tags: Vec<String>,
    new_tag: String,
    tag_filter: Option<String>,
    show_settings: bool,
    settings_projects_root: String,
    settings_preferred_ide: String,
    export_status: String,
    watch_rx: Option<Receiver<Vec<(Project, LanguageBreakdown)>>>,
    /// Parsed checklist per project_id, re-read on scan/watch (None = no list).
    task_cache: HashMap<String, Option<Checklist>>,
    /// Cached project-list rows + the inputs they were built for. Bumping
    /// `list_version` (on data changes) or changing search/sort/filter rebuilds.
    list_version: u64,
    list_cache: Vec<ListRow>,
    list_key: Option<(u64, String, SortMode, Option<String>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortMode {
    RecentActivity,
    MostStale,
    Alphabetical,
}

/// Tabs within the details panel for the selected project.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Overview,
    Roadmap,
    Notes,
}

/// Precomputed display data for one project-list row, so the list isn't rebuilt
/// (and tooltip strings re-allocated) on every frame — only when the underlying
/// data, search, sort, or filter changes.
struct ListRow {
    idx: usize,
    emoji: String,
    name: String,
    dirty: bool,
    icon_color: egui::Color32,
    pinned: bool,
    task_badge: String,
    task_color: egui::Color32,
    tip: String,
}

impl SortMode {
    fn label(&self) -> &'static str {
        match self {
            SortMode::RecentActivity => "Recent Activity",
            SortMode::MostStale => "Most Stale",
            SortMode::Alphabetical => "Alphabetical",
        }
    }
}

impl ProjectShelfApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);
        configure_style(&cc.egui_ctx);
        let db = Database::open().ok();
        let projects = db
            .as_ref()
            .and_then(|d| d.get_all_projects().ok())
            .unwrap_or_default();

        let all_tags = db
            .as_ref()
            .and_then(|d| d.get_all_tags().ok())
            .unwrap_or_default();

        let settings = load_app_settings();

        let mut app = Self {
            projects,
            languages: HashMap::new(),
            selected_idx: None,
            search_query: String::new(),
            db,
            scan_rx: None,
            scan_tx: None,
            is_scanning: false,
            sort_mode: SortMode::RecentActivity,
            detail_tab: DetailTab::Overview,
            user_meta: HashMap::new(),
            tags: HashMap::new(),
            all_tags,
            new_tag: String::new(),
            tag_filter: None,
            show_settings: false,
            settings_projects_root: settings.projects_root,
            settings_preferred_ide: settings.preferred_ide,
            export_status: String::new(),
            watch_rx: None,
            task_cache: HashMap::new(),
            list_version: 0,
            list_cache: Vec::new(),
            list_key: None,
        };

        app.start_scan();
        app.start_watcher(cc.egui_ctx.clone());
        app
    }

    /// Spawn a background thread that watches the projects root and pushes
    /// incremental re-scans of changed projects over a channel.
    fn start_watcher(&mut self, ctx: egui::Context) {
        let root = if self.settings_projects_root.is_empty() {
            config::projects_root()
        } else {
            PathBuf::from(&self.settings_projects_root)
        };
        self.watch_rx = Some(spawn_watcher(root, ctx));
    }

    fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        let (result_tx, result_rx) = channel();
        let (trigger_tx, _trigger_rx) = channel::<()>();

        self.scan_rx = Some(result_rx);
        self.scan_tx = Some(trigger_tx);
        self.is_scanning = true;

        let root = if self.settings_projects_root.is_empty() {
            config::projects_root()
        } else {
            std::path::PathBuf::from(&self.settings_projects_root)
        };

        thread::spawn(move || {
            let projects = scan_projects(&root);
            let _ = result_tx.send(projects);
        });
    }

    fn check_scan_results(&mut self) {
        if let Some(rx) = &self.scan_rx {
            if let Ok(scan_results) = rx.try_recv() {
                let mut projects = Vec::new();
                let mut languages = HashMap::new();

                for (project, lang_breakdown) in scan_results {
                    if let Some(db) = &self.db {
                        let _ = db.upsert_project(&project);
                        let _ = db.upsert_languages(&project.project_id, &lang_breakdown);
                    }

                    // Import notes from YAML on scan (DB is otherwise the source
                    // of truth — only fill in when the DB has no notes yet).
                    if let Some(pf) = ProjectFile::load(Path::new(&project.path)) {
                        let yaml_meta = pf.to_user_meta(&project.project_id);
                        if let Some(db) = &self.db {
                            if let Ok(existing) = db.get_user_meta(&project.project_id) {
                                if existing.notes.is_empty() && !yaml_meta.notes.is_empty() {
                                    let _ = db.upsert_user_meta(&yaml_meta);
                                }
                            }
                        }
                    }

                    languages.insert(project.project_id.clone(), lang_breakdown);
                    projects.push(project);
                }

                if let Some(db) = &self.db {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = db.update_scan_state(now);

                    self.all_tags = db.get_all_tags().unwrap_or_default();
                }

                self.projects = projects;
                self.languages = languages;
                self.user_meta.clear();
                self.tags.clear();
                self.task_cache.clear();
                self.list_version = self.list_version.wrapping_add(1);
                self.is_scanning = false;
                self.scan_rx = None;
            }
        }
    }

    /// Drain incremental updates from the file watcher and merge them into the
    /// in-memory project list (and DB), without a full rescan.
    fn check_watch_results(&mut self) {
        let updates: Vec<Vec<(Project, LanguageBreakdown)>> = match &self.watch_rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };

        if !updates.is_empty() {
            self.list_version = self.list_version.wrapping_add(1);
        }
        for batch in updates {
            for (project, breakdown) in batch {
                if let Some(db) = &self.db {
                    let _ = db.upsert_project(&project);
                    let _ = db.upsert_languages(&project.project_id, &breakdown);

                    // Import notes from YAML, mirroring check_scan_results.
                    if let Some(pf) = ProjectFile::load(Path::new(&project.path)) {
                        let yaml_meta = pf.to_user_meta(&project.project_id);
                        if let Ok(existing) = db.get_user_meta(&project.project_id) {
                            if existing.notes.is_empty() && !yaml_meta.notes.is_empty() {
                                let _ = db.upsert_user_meta(&yaml_meta);
                            }
                        }
                    }
                }

                self.languages.insert(project.project_id.clone(), breakdown);
                self.task_cache.remove(&project.project_id);
                match self
                    .projects
                    .iter()
                    .position(|p| p.project_id == project.project_id)
                {
                    Some(idx) => self.projects[idx] = project,
                    None => self.projects.push(project),
                }
            }
        }
    }

    fn filtered_project_indices(&self) -> Vec<usize> {
        let query = self.search_query.to_lowercase();
        let mut filtered: Vec<_> = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let text_match = query.is_empty()
                    || p.name.to_lowercase().contains(&query)
                    || p.path.to_lowercase().contains(&query);

                let tag_match = match &self.tag_filter {
                    Some(tag) => self
                        .tags
                        .get(&p.project_id)
                        .map(|t| t.contains(tag))
                        .unwrap_or(false),
                    None => true,
                };

                text_match && tag_match
            })
            .collect();

        match self.sort_mode {
            SortMode::RecentActivity => {
                filtered.sort_by(|a, b| b.1.activity_ts().cmp(&a.1.activity_ts()));
            }
            SortMode::MostStale => {
                filtered.sort_by(|a, b| a.1.activity_ts().cmp(&b.1.activity_ts()));
            }
            SortMode::Alphabetical => {
                filtered.sort_by(|a, b| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()));
            }
        }

        // Pinned projects float to top
        let pinned_set: HashSet<String> = self
            .user_meta
            .iter()
            .filter(|(_, m)| m.pinned)
            .map(|(id, _)| id.clone())
            .collect();

        filtered.sort_by(|a, b| {
            let a_pinned = pinned_set.contains(&a.1.project_id);
            let b_pinned = pinned_set.contains(&b.1.project_id);
            b_pinned.cmp(&a_pinned)
        });

        filtered.into_iter().map(|(idx, _)| idx).collect()
    }

    fn render_project_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Projects");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⚙").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });

        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.text_edit_singleline(&mut self.search_query);
        });

        ui.horizontal(|ui| {
            ui.label("Sort:");
            egui::ComboBox::from_id_source("sort_mode")
                .selected_text(self.sort_mode.label())
                .show_ui(ui, |ui: &mut egui::Ui| {
                    ui.selectable_value(
                        &mut self.sort_mode,
                        SortMode::RecentActivity,
                        "Recent Activity",
                    );
                    ui.selectable_value(&mut self.sort_mode, SortMode::MostStale, "Most Stale");
                    ui.selectable_value(
                        &mut self.sort_mode,
                        SortMode::Alphabetical,
                        "Alphabetical",
                    );
                });

            if self.is_scanning {
                ui.spinner();
            } else if ui.button("⟳ Refresh").clicked() {
                self.start_scan();
            }
            ui.menu_button("⬇ Export", |ui| {
                if ui.button("Markdown (.md)").clicked() {
                    self.export_report(ExportFormat::Markdown);
                    ui.close_menu();
                }
                if ui.button("CSV (.csv)").clicked() {
                    self.export_report(ExportFormat::Csv);
                    ui.close_menu();
                }
            });
        });

        if !self.export_status.is_empty() {
            ui.label(
                egui::RichText::new(&self.export_status)
                    .color(egui::Color32::from_rgb(100, 200, 100))
                    .small(),
            );
        }

        if !self.all_tags.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Tag:");
                let current = self.tag_filter.as_deref().unwrap_or("All");
                egui::ComboBox::from_id_source("tag_filter")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.tag_filter.is_none(), "All").clicked() {
                            self.tag_filter = None;
                        }
                        for tag in &self.all_tags.clone() {
                            let selected = self.tag_filter.as_deref() == Some(tag.as_str());
                            if ui.selectable_label(selected, tag).clicked() {
                                self.tag_filter = Some(tag.clone());
                            }
                        }
                    });
            });
        }

        ui.separator();

        // Load each project's tags once. Insert an entry for every project (even
        // when it has no tags) so the map becomes non-empty after the first pass —
        // otherwise, when no project has tags, this would re-query the DB for all
        // projects on every frame and make scrolling janky.
        if self.tags.is_empty() && !self.projects.is_empty() {
            if let Some(db) = &self.db {
                for p in &self.projects {
                    let t = db.get_tags(&p.project_id).unwrap_or_default();
                    self.tags.insert(p.project_id.clone(), t);
                }
            }
        }

        // Lazy-load user_meta for pinned sorting
        if self.user_meta.is_empty() {
            if let Some(db) = &self.db {
                for p in &self.projects {
                    if let Ok(meta) = db.get_user_meta(&p.project_id) {
                        self.user_meta.insert(p.project_id.clone(), meta);
                    }
                }
            }
        }

        // Rebuild the cached rows only when the data / search / sort / filter
        // changed — not every frame.
        let key = (
            self.list_version,
            self.search_query.clone(),
            self.sort_mode,
            self.tag_filter.clone(),
        );
        if self.list_key.as_ref() != Some(&key) {
            self.rebuild_list_rows();
            self.list_key = Some(key);
        }

        let mut new_selection = self.selected_idx;

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for row in &self.list_cache {
                    let is_selected = new_selection == Some(row.idx);
                    let resp = ui.horizontal(|ui| {
                        if row.pinned {
                            ui.label(egui::RichText::new("📌").size(12.0));
                        }
                        ui.label(egui::RichText::new(&row.emoji).color(row.icon_color).size(16.0))
                            .on_hover_ui(|ui| tooltip_body(ui, &row.tip));
                        if ui
                            .selectable_label(is_selected, &row.name)
                            .on_hover_ui(|ui| tooltip_body(ui, &row.tip))
                            .clicked()
                        {
                            new_selection = Some(row.idx);
                        }
                        if row.dirty {
                            ui.label(
                                egui::RichText::new("●")
                                    .color(egui::Color32::from_rgb(255, 150, 50)),
                            );
                        }
                        if !row.task_badge.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("☑ {}", row.task_badge))
                                    .color(row.task_color)
                                    .small(),
                            );
                        }
                    });
                    // Whole-row hover (covers the gaps between widgets too).
                    resp.response.on_hover_ui(|ui| tooltip_body(ui, &row.tip));
                }
            });

        self.selected_idx = new_selection;
    }

    /// Rebuild `list_cache` from the current projects / filter / sort. Called
    /// only when inputs change (tracked by `list_key`), so the per-row work —
    /// including building the tooltip strings — doesn't run every frame.
    fn rebuild_list_rows(&mut self) {
        let indices = self.filtered_project_indices();
        let mut rows = Vec::with_capacity(indices.len());
        for idx in indices {
            let Some(p) = self.projects.get(idx) else {
                continue;
            };
            let pinned = self
                .user_meta
                .get(&p.project_id)
                .map(|m| m.pinned)
                .unwrap_or(false);
            let task_badge = if p.tasks.total > 0 {
                format!("{}/{}", p.tasks.done, p.tasks.total)
            } else {
                String::new()
            };
            let task_color = roadmap_badge_color(p.tasks.done, p.tasks.total);
            let icon_color = match p.primary_language.as_deref() {
                Some(lang) => language_color(lang),
                None => icon_kind_color(p.icon_kind.as_str()),
            };

            let mut tip = p.name.clone();
            tip.push_str(&format!(
                "\nLanguage: {}",
                p.primary_language.as_deref().unwrap_or("—")
            ));
            if let Some(b) = &p.branch {
                tip.push_str(&format!("\nBranch: {b}"));
            }
            let when = p
                .last_commit_ts
                .map(format_timestamp)
                .unwrap_or_else(|| "—".to_string());
            tip.push_str(&format!("\nLast commit: {when}"));
            if p.tasks.total > 0 {
                tip.push_str(&format!("\nRoadmap: {}/{}", p.tasks.done, p.tasks.total));
            }
            if p.dirty {
                tip.push_str("\n● Uncommitted changes");
            }

            rows.push(ListRow {
                idx,
                emoji: p.icon_kind.emoji().to_string(),
                name: p.name.clone(),
                dirty: p.dirty,
                icon_color,
                pinned,
                task_badge,
                task_color,
                tip,
            });
        }
        self.list_cache = rows;
    }

    fn build_export_row(&self, p: &Project) -> ExportRow {
        let tags = self
            .db
            .as_ref()
            .and_then(|db| db.get_tags(&p.project_id).ok())
            .unwrap_or_default();
        ExportRow {
            name: p.name.clone(),
            path: p.path.clone(),
            language: p.primary_language.clone().unwrap_or_default(),
            branch: p.branch.clone().unwrap_or_default(),
            dirty: p.dirty,
            last_commit: p.last_commit_ts.map(format_timestamp).unwrap_or_default(),
            tags: tags.join(" "),
            health: p.health,
        }
    }

    /// Write `rows` to `<stem>.{md,csv}` in the app data dir, recording the
    /// outcome in `export_status`. `label` is used in the success message.
    fn write_export(&mut self, rows: &[ExportRow], format: ExportFormat, stem: &str, label: &str) {
        let content = match format {
            ExportFormat::Markdown => render_markdown(rows),
            ExportFormat::Csv => render_csv(rows),
        };
        let ext = match format {
            ExportFormat::Markdown => "md",
            ExportFormat::Csv => "csv",
        };
        let dir = config::data_dir();
        let out_path = dir.join(format!("{stem}.{ext}"));

        self.export_status = match std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&out_path, content))
        {
            Ok(()) => format!("Exported {label} → {}", out_path.display()),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    fn export_report(&mut self, format: ExportFormat) {
        let rows: Vec<ExportRow> = self
            .projects
            .iter()
            .map(|p| self.build_export_row(p))
            .collect();
        let label = format!("{} projects", rows.len());
        self.write_export(&rows, format, "projectshelf-report", &label);
    }

    fn export_single(&mut self, format: ExportFormat, project: &Project) {
        let row = self.build_export_row(project);
        let stem = format!("project-{}", sanitize_filename(&project.name));
        let label = project.name.clone();
        self.write_export(&[row], format, &stem, &label);
    }

    fn render_project_details(&mut self, ui: &mut egui::Ui) {
        let project = match self.selected_idx.and_then(|i| self.projects.get(i)) {
            Some(p) => p,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Select a project to view details");
                });
                return;
            }
        };

        let pid = project.project_id.clone();
        let is_pinned = self.user_meta.get(&pid).map(|m| m.pinned).unwrap_or(false);
        let mut toggle_pin = false;
        ui.horizontal(|ui| {
            ui.heading(format!("{} {}", project.icon_kind.emoji(), project.name));
            if project.dirty {
                ui.label(egui::RichText::new("●").color(egui::Color32::from_rgb(255, 150, 50)));
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(&project.path)
                    .color(egui::Color32::GRAY)
                    .small(),
            );
            // Pin floats to the top-right of the panel.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pin_label = if is_pinned { "📌 Pinned" } else { "📌 Pin" };
                if ui.selectable_label(is_pinned, pin_label).clicked() {
                    toggle_pin = true;
                }
            });
        });

        if toggle_pin {
            let meta = self.user_meta.entry(pid.clone()).or_insert_with(|| UserMeta {
                project_id: pid.clone(),
                pinned: false,
                notes: String::new(),
            });
            meta.pinned = !meta.pinned;
            if let Some(db) = &self.db {
                let _ = db.upsert_user_meta(meta);
            }
            self.list_version = self.list_version.wrapping_add(1);
        }

        ui.separator();

        ui.horizontal(|ui| {
            if colored_button(ui, "📂 Open Folder", egui::Color32::from_rgb(100, 149, 237)).clicked() {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&project.path)
                    .spawn();
            }
            if colored_button(ui, "💻 Terminal", egui::Color32::from_rgb(80, 80, 80)).clicked() {
                let _ = std::process::Command::new("x-terminal-emulator")
                    .current_dir(&project.path)
                    .spawn()
                    .or_else(|_| {
                        std::process::Command::new("gnome-terminal")
                            .arg("--working-directory")
                            .arg(&project.path)
                            .spawn()
                    })
                    .or_else(|_| {
                        std::process::Command::new("konsole")
                            .arg("--workdir")
                            .arg(&project.path)
                            .spawn()
                    });
            }
            if colored_button(ui, "📝 Open in IDE", egui::Color32::from_rgb(86, 156, 214)).clicked() {
                let path = &project.path;
                let preferred = &self.settings_preferred_ide;
                if !preferred.is_empty() {
                    let _ = std::process::Command::new(preferred)
                        .arg(path)
                        .spawn()
                        .or_else(|_| std::process::Command::new("code").arg(path).spawn());
                } else {
                    let _ = std::process::Command::new("windsurf")
                        .arg(path)
                        .spawn()
                        .or_else(|_| std::process::Command::new("cursor").arg(path).spawn())
                        .or_else(|_| std::process::Command::new("code").arg(path).spawn())
                        .or_else(|_| std::process::Command::new("codium").arg(path).spawn())
                        .or_else(|_| std::process::Command::new("subl").arg(path).spawn())
                        .or_else(|_| std::process::Command::new("atom").arg(path).spawn())
                        .or_else(|_| std::process::Command::new("gedit").arg(path).spawn());
                }
            }
            if let Some(github_url) = &project.github_url {
                if colored_button(ui, "🔗 GitHub", egui::Color32::from_rgb(110, 84, 148)).clicked() {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(github_url)
                        .spawn();
                }
            }
        });

        ui.add_space(6.0);

        // Per-project tabs. Own the project so the tab bodies can take `&mut self`.
        let project = project.clone();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.detail_tab, DetailTab::Overview, "Overview");
            ui.selectable_value(&mut self.detail_tab, DetailTab::Roadmap, "Roadmap");
            ui.selectable_value(&mut self.detail_tab, DetailTab::Notes, "Notes");
        });
        ui.separator();

        match self.detail_tab {
            DetailTab::Overview => self.render_overview(ui, &project),
            DetailTab::Roadmap => self.render_roadmap(ui, &project.project_id, &project.path),
            DetailTab::Notes => self.render_notes(ui, &project.project_id, &project.path),
        }
    }

    /// "Overview" tab: activity + health side by side, languages, export, and a
    /// collapsed tags editor.
    fn render_overview(&mut self, ui: &mut egui::Ui, project: &Project) {
        // Activity (left) and Health (right), side by side.
        ui.columns(2, |cols| {
            cols[0].heading("Activity");
            egui::Grid::new("activity_grid")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(&mut cols[0], |ui| {
                    ui.label("Last Seen:");
                    ui.label(format_timestamp(project.last_seen));
                    ui.end_row();

                    if let Some(ts) = project.last_commit_ts {
                        ui.label("Last Commit:");
                        ui.label(format_timestamp(ts));
                        ui.end_row();
                    }

                    if let Some(ts) = project.last_fs_activity_ts {
                        ui.label("Last FS Activity:");
                        ui.label(format_timestamp(ts));
                        ui.end_row();
                    }

                    if let Some(branch) = &project.branch {
                        ui.label("Branch:");
                        ui.label(branch);
                        ui.end_row();
                    }

                    ui.label("Dirty:");
                    if project.dirty {
                        ui.label(
                            egui::RichText::new("Yes ●")
                                .color(egui::Color32::from_rgb(255, 150, 50)),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("No ✓")
                                .color(egui::Color32::from_rgb(100, 200, 100)),
                        );
                    }
                    ui.end_row();
                });

            cols[1].heading("Health");
            let health = project.health;
            health_badge(&mut cols[1], "README", health.has_readme);
            health_badge(&mut cols[1], "License", health.has_license);
            health_badge(&mut cols[1], "Tests", health.has_tests);
            health_badge(&mut cols[1], "CI", health.has_ci);
        });

        ui.add_space(4.0);
        ui.separator();
        ui.heading("Languages");

        if let Some(breakdown) = self.languages.get(&project.project_id) {
            let top_langs = breakdown.top_n(5);
            if top_langs.is_empty() {
                ui.label("No code files detected");
            } else {
                for (lang, bytes, pct) in top_langs {
                    ui.horizontal(|ui| {
                        let bar_width = 100.0;
                        let filled_width = bar_width * (pct / 100.0);

                        let (rect, _response) = ui.allocate_exact_size(
                            egui::vec2(bar_width, 14.0),
                            egui::Sense::hover(),
                        );

                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            egui::Color32::from_rgb(60, 60, 60),
                        );

                        let filled_rect = egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(filled_width, rect.height()),
                        );
                        ui.painter().rect_filled(
                            filled_rect,
                            2.0,
                            language_color(lang),
                        );

                        ui.label(format!("{} ({:.1}%)", lang, pct));
                        ui.label(egui::RichText::new(format_bytes(bytes)).color(egui::Color32::GRAY));
                    });
                }
            }
        } else if let Some(lang) = &project.primary_language {
            ui.label(lang);
        } else {
            ui.label("No language data");
        }

        ui.add_space(6.0);
        ui.menu_button("⬇ Export", |ui| {
            if ui.button("Markdown (.md)").clicked() {
                self.export_single(ExportFormat::Markdown, project);
                ui.close_menu();
            }
            if ui.button("CSV (.csv)").clicked() {
                self.export_single(ExportFormat::Csv, project);
                ui.close_menu();
            }
        });

        ui.add_space(8.0);
        egui::CollapsingHeader::new("Tags")
            .default_open(false)
            .show(ui, |ui| {
                self.render_tags(ui, &project.project_id);
            });
    }

    /// Tags editor for a project. Tucked into a collapsed section at the bottom
    /// of the details panel — low-priority for now.
    fn render_tags(&mut self, ui: &mut egui::Ui, project_id: &str) {
        let pid = project_id.to_string();
        if !self.tags.contains_key(&pid) {
            if let Some(db) = &self.db {
                if let Ok(t) = db.get_tags(&pid) {
                    self.tags.insert(pid.clone(), t);
                }
            }
        }

        let current_tags: Vec<String> = self.tags.get(&pid).cloned().unwrap_or_default();

        ui.horizontal_wrapped(|ui| {
            let mut tag_to_remove = None;
            for tag in &current_tags {
                ui.label(
                    egui::RichText::new(tag)
                        .background_color(egui::Color32::from_rgb(60, 80, 120))
                        .color(egui::Color32::from_rgb(200, 210, 230)),
                );
                if ui.small_button("×").clicked() {
                    tag_to_remove = Some(tag.clone());
                }
            }
            if let Some(tag) = tag_to_remove {
                if let Some(db) = &self.db {
                    let _ = db.remove_tag(&pid, &tag);
                }
                if let Some(t) = self.tags.get_mut(&pid) {
                    t.retain(|x| x != &tag);
                }
                self.all_tags = self
                    .db
                    .as_ref()
                    .and_then(|d| d.get_all_tags().ok())
                    .unwrap_or_default();
                self.list_version = self.list_version.wrapping_add(1);
            }
        });

        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.new_tag);
            if ui.small_button("+ Tag").clicked() && !self.new_tag.trim().is_empty() {
                let tag = self.new_tag.trim().to_lowercase();
                if let Some(db) = &self.db {
                    let _ = db.add_tag(&pid, &tag);
                }
                self.tags.entry(pid.clone()).or_default().push(tag.clone());
                if !self.all_tags.contains(&tag) {
                    self.all_tags.push(tag);
                    self.all_tags.sort();
                }
                self.new_tag.clear();
                self.list_version = self.list_version.wrapping_add(1);
            }
        });
    }

    /// Read-only view of the project's `ROADMAP.md` checklist. The file is the
    /// source of truth; we never write back. Parsed result is cached per project
    /// and invalidated on scan/watch. The item list is independently scrollable.
    fn render_roadmap(&mut self, ui: &mut egui::Ui, project_id: &str, project_path: &str) {
        let checklist = self
            .task_cache
            .entry(project_id.to_string())
            .or_insert_with(|| parse_checklist(Path::new(project_path)));

        let checklist = match checklist {
            Some(c) => c,
            None => {
                ui.add_space(2.0);
                ui.heading("Roadmap");
                ui.label(
                    egui::RichText::new("No ROADMAP.md found")
                        .color(egui::Color32::from_rgb(130, 130, 130))
                        .italics(),
                );
                return;
            }
        };

        ui.horizontal(|ui| {
            ui.heading("Roadmap");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&checklist.source)
                        .color(egui::Color32::from_rgb(120, 120, 120))
                        .small(),
                );
            });
        });

        // Progress summary + bar.
        let total = checklist.total();
        let done = checklist.done();
        let pct = if total > 0 {
            (done as f32 / total as f32 * 100.0).round() as u32
        } else {
            0
        };
        ui.add_space(4.0);
        let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
        let bar_w = ui.available_width().min(380.0);
        let (rect, _r) = ui.allocate_exact_size(egui::vec2(bar_w, 10.0), egui::Sense::hover());
        let rounding = egui::Rounding::same(5.0);
        ui.painter()
            .rect_filled(rect, rounding, egui::Color32::from_rgb(48, 50, 54));
        if frac > 0.0 {
            let filled =
                egui::Rect::from_min_size(rect.min, egui::vec2(bar_w * frac, rect.height()));
            ui.painter()
                .rect_filled(filled, rounding, egui::Color32::from_rgb(96, 176, 112));
        }
        ui.add_space(3.0);
        ui.label(
            egui::RichText::new(format!("{done} / {total} done · {pct}%"))
                .color(egui::Color32::from_rgb(165, 165, 170))
                .small(),
        );
        ui.add_space(6.0);

        // Scrollable item list, filling the remaining height of the Roadmap tab.
        egui::ScrollArea::vertical()
            .id_source(("roadmap", project_id))
            .max_height(ui.available_height().max(160.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                // Uncompleted items first, grouped by their section.
                let mut any_open = false;
                let mut first = true;
                for (i, section) in checklist.sections.iter().enumerate() {
                    if !section.items.iter().any(|it| !it.done) {
                        continue;
                    }
                    any_open = true;
                    if let Some(title) = &section.title {
                        if !first {
                            ui.add_space(8.0);
                        }
                        let (text, color) = if section.counted {
                            (title.clone(), egui::Color32::from_rgb(214, 214, 220))
                        } else {
                            (
                                format!("{title}  ·  backlog"),
                                egui::Color32::from_rgb(135, 135, 140),
                            )
                        };
                        ui.label(egui::RichText::new(text).strong().color(color));
                        ui.add_space(3.0);
                    }
                    first = false;
                    ui.indent(("todo", project_id, i), |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for item in section.items.iter().filter(|it| !it.done) {
                            ui.label(
                                egui::RichText::new(format!("○  {}", item.text))
                                    .color(egui::Color32::from_rgb(205, 205, 212)),
                            );
                        }
                    });
                }
                if !any_open {
                    ui.label(
                        egui::RichText::new("✓  All tasks complete")
                            .color(egui::Color32::from_rgb(120, 190, 130)),
                    );
                }

                // Completed items, split out below and collapsed by default.
                let done_total = checklist
                    .sections
                    .iter()
                    .flat_map(|s| &s.items)
                    .filter(|it| it.done)
                    .count();
                if done_total > 0 {
                    ui.add_space(10.0);
                    egui::CollapsingHeader::new(format!("Completed ({done_total})"))
                        .id_source(("completed", project_id))
                        .default_open(false)
                        .show(ui, |ui| {
                            for (i, section) in checklist.sections.iter().enumerate() {
                                if !section.items.iter().any(|it| it.done) {
                                    continue;
                                }
                                if let Some(title) = &section.title {
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(title)
                                            .strong()
                                            .color(egui::Color32::from_rgb(150, 150, 156)),
                                    );
                                    ui.add_space(2.0);
                                }
                                ui.indent(("done", project_id, i), |ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    for item in section.items.iter().filter(|it| it.done) {
                                        ui.label(
                                            egui::RichText::new(format!("✓  {}", item.text))
                                                .color(egui::Color32::from_rgb(120, 128, 122))
                                                .strikethrough(),
                                        );
                                    }
                                });
                            }
                        });
                }
            });
    }

    fn render_notes(&mut self, ui: &mut egui::Ui, project_id: &str, project_path: &str) {
        ui.horizontal(|ui| {
            ui.heading("Notes");
            if ui.small_button("💾 Save to file").clicked() {
                self.save_project_file(project_id, project_path);
            }
        });

        if !self.user_meta.contains_key(project_id) {
            if let Some(db) = &self.db {
                if let Ok(meta) = db.get_user_meta(project_id) {
                    self.user_meta.insert(project_id.to_string(), meta);
                }
            }
        }

        let meta = self
            .user_meta
            .entry(project_id.to_string())
            .or_insert_with(|| UserMeta {
                project_id: project_id.to_string(),
                pinned: false,
                notes: String::new(),
            });

        let response = ui.add(
            egui::TextEdit::multiline(&mut meta.notes)
                .desired_width(f32::INFINITY)
                .desired_rows(4)
                .hint_text("Add notes about this project..."),
        );

        if response.changed() {
            if let Some(db) = &self.db {
                let _ = db.upsert_user_meta(meta);
            }
        }
    }

    fn save_project_file(&self, project_id: &str, project_path: &str) {
        let user_meta = self
            .user_meta
            .get(project_id)
            .cloned()
            .unwrap_or_else(|| UserMeta {
                project_id: project_id.to_string(),
                pinned: false,
                notes: String::new(),
            });

        // Preserve any existing milestones in the YAML; only update notes/pinned.
        let mut proj_file = ProjectFile::load(Path::new(project_path)).unwrap_or_default();
        proj_file.notes = user_meta.notes;
        proj_file.pinned = user_meta.pinned;
        let _ = proj_file.save(Path::new(project_path));
    }

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();

        ui.label("Projects Root Directory:");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.settings_projects_root);
        });
        ui.label(
            egui::RichText::new("Leave empty to use default ~/Projects")
                .color(egui::Color32::GRAY)
                .small(),
        );

        ui.add_space(10.0);

        ui.label("Preferred IDE (command name):");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.settings_preferred_ide);
        });
        ui.label(
            egui::RichText::new("e.g. windsurf, cursor, code, codium, subl")
                .color(egui::Color32::GRAY)
                .small(),
        );

        ui.add_space(15.0);

        ui.horizontal(|ui| {
            if ui.button("💾 Save Settings").clicked() {
                save_app_settings(&AppSettings {
                    projects_root: self.settings_projects_root.clone(),
                    preferred_ide: self.settings_preferred_ide.clone(),
                });
            }
            if ui.button("Close").clicked() {
                self.show_settings = false;
            }
        });
    }
}

impl eframe::App for ProjectShelfApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_scan_results();
        self.check_watch_results();

        if self.is_scanning {
            ctx.request_repaint();
        }

        egui::SidePanel::left("project_list")
            .resizable(true)
            .default_width(300.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                self.render_project_list(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_settings {
                self.render_settings(ui);
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        self.render_project_details(ui);
                    });
            }
        });
    }
}

fn format_timestamp(ts: i64) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let time = UNIX_EPOCH + Duration::from_secs(ts as u64);
    let now = SystemTime::now();

    if let Ok(elapsed) = now.duration_since(time) {
        let secs = elapsed.as_secs();
        if secs < 60 {
            return "just now".to_string();
        }
        let mins = secs / 60;
        if mins < 60 {
            return format!("{} min ago", mins);
        }
        let hours = mins / 60;
        if hours < 24 {
            return format!("{} hours ago", hours);
        }
        let days = hours / 24;
        if days < 30 {
            return format!("{} days ago", days);
        }
        let months = days / 30;
        if months < 12 {
            return format!("{} months ago", months);
        }
        let years = months / 12;
        return format!("{} years ago", years);
    }

    "unknown".to_string()
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{:.1} KB", kb);
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{:.1} MB", mb);
    }
    let gb = mb / 1024.0;
    format!("{:.1} GB", gb)
}

fn language_color(lang: &str) -> egui::Color32 {
    match lang {
        "Rust" => egui::Color32::from_rgb(222, 165, 132),
        "Python" => egui::Color32::from_rgb(53, 114, 165),
        "JavaScript" => egui::Color32::from_rgb(241, 224, 90),
        "TypeScript" => egui::Color32::from_rgb(49, 120, 198),
        "Go" => egui::Color32::from_rgb(0, 173, 216),
        "C" => egui::Color32::from_rgb(85, 85, 85),
        "C++" => egui::Color32::from_rgb(243, 75, 125),
        "Java" => egui::Color32::from_rgb(176, 114, 25),
        "Ruby" => egui::Color32::from_rgb(204, 52, 45),
        "PHP" => egui::Color32::from_rgb(79, 93, 149),
        "C#" => egui::Color32::from_rgb(23, 134, 0),
        "Swift" => egui::Color32::from_rgb(255, 172, 69),
        "Kotlin" => egui::Color32::from_rgb(169, 123, 255),
        "Shell" => egui::Color32::from_rgb(137, 224, 81),
        "HTML" => egui::Color32::from_rgb(227, 76, 38),
        "CSS" => egui::Color32::from_rgb(86, 61, 124),
        "SCSS" => egui::Color32::from_rgb(198, 83, 140),
        "Vue" => egui::Color32::from_rgb(65, 184, 131),
        "Svelte" => egui::Color32::from_rgb(255, 62, 0),
        "Markdown" => egui::Color32::from_rgb(8, 63, 161),
        "JSON" => egui::Color32::from_rgb(41, 41, 41),
        "YAML" => egui::Color32::from_rgb(203, 23, 30),
        "TOML" => egui::Color32::from_rgb(156, 66, 33),
        _ => egui::Color32::from_rgb(100, 100, 100),
    }
}

fn icon_kind_color(kind: &str) -> egui::Color32 {
    match kind {
        "rust" => egui::Color32::from_rgb(222, 165, 132),
        "python" => egui::Color32::from_rgb(55, 118, 171),
        "node" => egui::Color32::from_rgb(104, 159, 99),
        "go" => egui::Color32::from_rgb(0, 173, 216),
        "cpp" => egui::Color32::from_rgb(243, 75, 125),
        "git" => egui::Color32::from_rgb(240, 80, 51),
        "marked" => egui::Color32::from_rgb(255, 193, 7),
        _ => egui::Color32::from_rgb(150, 150, 150),
    }
}

fn colored_button(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    let button = egui::Button::new(
        egui::RichText::new(text).color(egui::Color32::from_rgb(230, 230, 230))
    ).fill(color);
    ui.add(button)
}

#[derive(Clone, Copy)]
enum ExportFormat {
    Markdown,
    Csv,
}

struct ExportRow {
    name: String,
    path: String,
    language: String,
    branch: String,
    dirty: bool,
    last_commit: String,
    tags: String,
    health: projectshelf_core::HealthChecks,
}

const EXPORT_HEADERS: [&str; 11] = [
    "Name", "Path", "Language", "Branch", "Dirty", "Last Commit", "Tags", "README", "License",
    "Tests", "CI",
];

fn export_cells(r: &ExportRow) -> [String; 11] {
    let yn = |b: bool| if b { "yes" } else { "no" }.to_string();
    [
        r.name.clone(),
        r.path.clone(),
        r.language.clone(),
        r.branch.clone(),
        yn(r.dirty),
        r.last_commit.clone(),
        r.tags.clone(),
        yn(r.health.has_readme),
        yn(r.health.has_license),
        yn(r.health.has_tests),
        yn(r.health.has_ci),
    ]
}

fn render_markdown(rows: &[ExportRow]) -> String {
    let mut out = String::from("# ProjectShelf Report\n\n");
    out.push_str(&format!("| {} |\n", EXPORT_HEADERS.join(" | ")));
    out.push_str(&format!("|{}\n", " --- |".repeat(EXPORT_HEADERS.len())));
    for r in rows {
        let cells: Vec<String> = export_cells(r)
            .iter()
            .map(|c| c.replace('|', "\\|"))
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

fn render_csv(rows: &[ExportRow]) -> String {
    let mut out = String::new();
    out.push_str(&EXPORT_HEADERS.join(","));
    out.push('\n');
    for r in rows {
        let cells: Vec<String> = export_cells(r).iter().map(|c| csv_escape(c)).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Spawn the filesystem watcher thread. Returns a receiver over which batches
/// of re-scanned (changed) projects arrive. The watcher debounces bursts and
/// only re-scans the projects whose files actually changed.
fn spawn_watcher(root: PathBuf, ctx: egui::Context) -> Receiver<Vec<(Project, LanguageBreakdown)>> {
    let (update_tx, update_rx) = channel::<Vec<(Project, LanguageBreakdown)>>();

    thread::spawn(move || {
        let (event_tx, event_rx) = channel::<notify::Event>();
        let mut watcher = match notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = event_tx.send(event);
                }
            },
        ) {
            Ok(w) => w,
            Err(_) => return,
        };
        // Lightweight watching: only each project's root dir (non-recursive)
        // and its `.git` dir, instead of recursively watching the whole tree.
        // Recursive watching of ~/Projects would register one inotify watch per
        // directory — ~100k of them, most inside node_modules/target/.git — which
        // is a huge resource cost and a likely cause of crashes under FS bursts.
        // Trade-off: edits deep inside a project (e.g. src/) won't auto-refresh;
        // commits, dirty state, and project-root changes still do. Use Refresh
        // for a full re-scan.
        let mut watch_count = 0;
        for d in discover_projects(&root) {
            if watcher.watch(&d.path, RecursiveMode::NonRecursive).is_ok() {
                watch_count += 1;
            }
            let git = d.path.join(".git");
            if git.is_dir() && watcher.watch(&git, RecursiveMode::NonRecursive).is_ok() {
                watch_count += 1;
            }
        }
        if watch_count == 0 {
            return;
        }

        loop {
            // Block until something changes, then drain a quiet window so a
            // burst of events (e.g. a git commit or save-all) collapses into
            // one re-scan.
            let first = match event_rx.recv() {
                Ok(e) => e,
                Err(_) => return,
            };
            let mut changed: HashSet<PathBuf> = HashSet::new();
            collect_changed_paths(&first, &mut changed);
            while let Ok(e) = event_rx.recv_timeout(Duration::from_millis(600)) {
                collect_changed_paths(&e, &mut changed);
            }

            let mut project_dirs: HashSet<PathBuf> = HashSet::new();
            for p in &changed {
                if let Some(dir) = find_project_root(p, &root) {
                    project_dirs.insert(dir);
                }
            }
            if project_dirs.is_empty() {
                continue;
            }

            let results: Vec<_> = project_dirs.iter().map(|d| scan_single_project(d)).collect();
            if update_tx.send(results).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    });

    update_rx
}

/// Record an event's paths, skipping build/vendor dirs but keeping `.git` so
/// commit and dirty-state changes are still picked up.
fn collect_changed_paths(event: &notify::Event, changed: &mut HashSet<PathBuf>) {
    for path in &event.paths {
        let in_ignored_dir = path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s != ".git" && IGNORED_DIRS.contains(&s.as_ref())
        });
        if !in_ignored_dir {
            changed.insert(path.clone());
        }
    }
}

/// Reduce a project name to a safe filename stem (alphanumerics, `-`, `_`).
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Color for the list roadmap badge by completion: amber when there's lots to
/// do, blue when almost done, green when complete.
fn roadmap_badge_color(done: u32, total: u32) -> egui::Color32 {
    let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
    if total > 0 && done >= total {
        egui::Color32::from_rgb(120, 190, 130) // done
    } else if frac >= 0.70 {
        egui::Color32::from_rgb(110, 160, 240) // almost done
    } else {
        egui::Color32::from_rgb(225, 190, 90) // lots to do
    }
}

/// Render a multi-line project tooltip with spaced-out lines (first line — the
/// project name — emphasized).
fn tooltip_body(ui: &mut egui::Ui, tip: &str) {
    ui.spacing_mut().item_spacing.y = 5.0;
    for (i, line) in tip.lines().enumerate() {
        if i == 0 {
            ui.label(egui::RichText::new(line).strong());
        } else if let Some(rest) = line.strip_prefix("● ") {
            // Dirty-state line: paint the marker (font may lack the ● glyph).
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(2.0),
                    egui::Color32::from_rgb(255, 150, 50),
                );
                ui.label(rest);
            });
        } else if let Some(rest) = line.strip_prefix("Roadmap: ") {
            // Color the roadmap line by completion, matching the list badge.
            let color = rest.split_once('/').and_then(|(d, t)| {
                let d = d.trim().parse::<u32>().ok()?;
                let t = t.trim().parse::<u32>().ok()?;
                Some(roadmap_badge_color(d, t))
            });
            match color {
                Some(c) => ui.label(egui::RichText::new(line).color(c)),
                None => ui.label(line),
            };
        } else {
            ui.label(line);
        }
    }
}

fn health_badge(ui: &mut egui::Ui, label: &str, present: bool) {
    let (mark, color) = if present {
        ("✓", egui::Color32::from_rgb(100, 200, 100))
    } else {
        ("✗", egui::Color32::from_rgb(150, 150, 150))
    };
    ui.label(
        egui::RichText::new(format!("{mark} {label}"))
            .color(color)
            .background_color(egui::Color32::from_rgb(45, 45, 45)),
    );
}

struct AppSettings {
    projects_root: String,
    preferred_ide: String,
}

fn settings_path() -> std::path::PathBuf {
    config::data_dir().join("settings.toml")
}

fn load_app_settings() -> AppSettings {
    let path = settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        let mut projects_root = String::new();
        let mut preferred_ide = String::new();
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("projects_root = ") {
                projects_root = val.trim_matches('"').to_string();
            }
            if let Some(val) = line.strip_prefix("preferred_ide = ") {
                preferred_ide = val.trim_matches('"').to_string();
            }
        }
        AppSettings {
            projects_root,
            preferred_ide,
        }
    } else {
        AppSettings {
            projects_root: config::projects_root().to_string_lossy().to_string(),
            preferred_ide: String::new(),
        }
    }
}

fn save_app_settings(settings: &AppSettings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = format!(
        "projects_root = \"{}\"\npreferred_ide = \"{}\"\n",
        settings.projects_root, settings.preferred_ide
    );
    let _ = std::fs::write(path, content);
}

/// Global spacing / visual tweaks to give the UI more breathing room.
fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.indent = 16.0;
    style.spacing.interact_size.y = 24.0;
    // Cleaner indents (no left guide-line on the roadmap item groups).
    style.visuals.indent_has_left_vline = false;
    // Lighter border on tooltips / popups / menus so it's easier to see.
    style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(140));
    // More breathing room inside tooltips / menus.
    style.spacing.menu_margin = egui::Margin::same(8.0);
    // Slightly rounder widgets.
    let rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.noninteractive.rounding = rounding;
    style.visuals.widgets.inactive.rounding = rounding;
    style.visuals.widgets.hovered.rounding = rounding;
    style.visuals.widgets.active.rounding = rounding;
    ctx.set_style(style);
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let candidates = [
        "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
        "/usr/share/fonts/opentype/noto/NotoColorEmoji.ttf",
        "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf",
        "/usr/share/fonts/truetype/ancient-scripts/Symbola.ttf",
        "/usr/share/fonts/truetype/ttf-ancient-fonts/Symbola.ttf",
    ];

    if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert(
            "emoji".to_string(),
            egui::FontData::from_owned(bytes),
        );

        if let Some(family) = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
        {
            family.push("emoji".to_string());
        }

        if let Some(family) = fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
        {
            family.push("emoji".to_string());
        }

        ctx.set_fonts(fonts);
    }
}

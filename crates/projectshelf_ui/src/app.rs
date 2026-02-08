use eframe::egui;
use projectshelf_core::{
    config, scan_projects, Database, LanguageBreakdown, Milestone, MilestoneStatus, Project,
    ProjectFile, UserMeta,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

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
    milestones: HashMap<String, Vec<Milestone>>,
    user_meta: HashMap<String, UserMeta>,
    new_milestone_title: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortMode {
    RecentActivity,
    MostStale,
    Alphabetical,
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
        let db = Database::open().ok();
        let projects = db
            .as_ref()
            .and_then(|d| d.get_all_projects().ok())
            .unwrap_or_default();

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
            milestones: HashMap::new(),
            user_meta: HashMap::new(),
            new_milestone_title: String::new(),
        };

        app.start_scan();
        app
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

        thread::spawn(move || {
            let root = config::projects_root();
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
                    languages.insert(project.project_id.clone(), lang_breakdown);
                    projects.push(project);
                }

                if let Some(db) = &self.db {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = db.update_scan_state(now);
                }

                self.projects = projects;
                self.languages = languages;
                self.is_scanning = false;
                self.scan_rx = None;
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
                query.is_empty()
                    || p.name.to_lowercase().contains(&query)
                    || p.path.to_lowercase().contains(&query)
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

        filtered.into_iter().map(|(idx, _)| idx).collect()
    }

    fn render_project_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Projects");

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
        });

        ui.separator();

        let filtered_indices = self.filtered_project_indices();
        let display_items: Vec<_> = filtered_indices
            .iter()
            .filter_map(|&idx| {
                self.projects.get(idx).map(|p| {
                    let dirty_badge = if p.dirty { " ●" } else { "" };
                    let branch_info = p.branch.as_deref().unwrap_or("");
                    (idx, p.icon_kind.emoji().to_string(), p.name.clone(), dirty_badge.to_string(), branch_info.to_string(), p.icon_kind.as_str().to_string())
                })
            })
            .collect();

        let mut new_selection = self.selected_idx;

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for (idx, emoji, name, dirty, branch, icon_kind) in &display_items {
                    let is_selected = new_selection == Some(*idx);
                    ui.horizontal(|ui| {
                        let icon_color = icon_kind_color(icon_kind);
                        ui.label(egui::RichText::new(emoji).color(icon_color).size(16.0));
                        let label_text = if branch.is_empty() {
                            name.clone()
                        } else {
                            format!("{} [{}]", name, branch)
                        };
                        let response = ui.selectable_label(is_selected, &label_text);
                        if !dirty.is_empty() {
                            ui.label(egui::RichText::new(dirty).color(egui::Color32::from_rgb(255, 150, 50)));
                        }
                        if response.clicked() {
                            new_selection = Some(*idx);
                        }
                    });
                }
            });

        self.selected_idx = new_selection;
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

        ui.horizontal(|ui| {
            ui.heading(format!("{} {}", project.icon_kind.emoji(), project.name));
            if project.dirty {
                ui.label(egui::RichText::new(" ●").color(egui::Color32::from_rgb(255, 150, 50)));
            }
        });
        ui.label(egui::RichText::new(&project.path).color(egui::Color32::GRAY));

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
            if let Some(github_url) = &project.github_url {
                if colored_button(ui, "🔗 GitHub", egui::Color32::from_rgb(110, 84, 148)).clicked() {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(github_url)
                        .spawn();
                }
            }
        });

        ui.separator();

        ui.heading("Activity");
        egui::Grid::new("activity_grid")
            .num_columns(2)
            .spacing([20.0, 4.0])
            .show(ui, |ui| {
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
                    ui.label(egui::RichText::new("Yes ●").color(egui::Color32::from_rgb(255, 150, 50)));
                } else {
                    ui.label(egui::RichText::new("No ✓").color(egui::Color32::from_rgb(100, 200, 100)));
                }
                ui.end_row();
            });

        ui.separator();
        ui.heading("Languages");

        let project_id = &project.project_id;
        if let Some(breakdown) = self.languages.get(project_id) {
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

        let project_id = project.project_id.clone();
        let project_path = project.path.clone();

        ui.separator();
        self.render_milestones(ui, &project_id);

        ui.separator();
        self.render_notes(ui, &project_id, &project_path);
    }

    fn render_milestones(&mut self, ui: &mut egui::Ui, project_id: &str) {
        ui.horizontal(|ui| {
            ui.heading("Milestones");
            if ui.small_button("+ Add").clicked() && !self.new_milestone_title.trim().is_empty() {
                let milestone = Milestone {
                    project_id: project_id.to_string(),
                    id: uuid_simple(),
                    title: self.new_milestone_title.trim().to_string(),
                    status: MilestoneStatus::Todo,
                    due_ts: None,
                    link: None,
                };
                if let Some(db) = &self.db {
                    let _ = db.upsert_milestone(&milestone);
                }
                self.milestones
                    .entry(project_id.to_string())
                    .or_default()
                    .push(milestone);
                self.new_milestone_title.clear();
            }
        });

        ui.horizontal(|ui| {
            ui.label("New:");
            ui.text_edit_singleline(&mut self.new_milestone_title);
        });

        if !self.milestones.contains_key(project_id) {
            if let Some(db) = &self.db {
                if let Ok(ms) = db.get_milestones(project_id) {
                    self.milestones.insert(project_id.to_string(), ms);
                }
            }
        }

        let milestones = self.milestones.get(project_id).cloned().unwrap_or_default();
        if milestones.is_empty() {
            ui.label(egui::RichText::new("No milestones yet").color(egui::Color32::GRAY));
        } else {
            let mut to_delete: Option<String> = None;
            let mut updates: Vec<Milestone> = Vec::new();

            for m in &milestones {
                ui.horizontal(|ui| {
                    let status_emoji = match m.status {
                        MilestoneStatus::Todo => "⬚",
                        MilestoneStatus::InProgress => "▶",
                        MilestoneStatus::Blocked => "⊘",
                        MilestoneStatus::Done => "✓",
                    };

                    let mut current_status = m.status.clone();
                    egui::ComboBox::from_id_source(&m.id)
                        .selected_text(status_emoji)
                        .width(40.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut current_status, MilestoneStatus::Todo, "⬚ Todo");
                            ui.selectable_value(&mut current_status, MilestoneStatus::InProgress, "▶ In Progress");
                            ui.selectable_value(&mut current_status, MilestoneStatus::Blocked, "⊘ Blocked");
                            ui.selectable_value(&mut current_status, MilestoneStatus::Done, "✓ Done");
                        });

                    if current_status != m.status {
                        let mut updated = m.clone();
                        updated.status = current_status;
                        updates.push(updated);
                    }

                    let text_color = match m.status {
                        MilestoneStatus::Done => egui::Color32::from_rgb(100, 200, 100),
                        MilestoneStatus::Blocked => egui::Color32::from_rgb(255, 100, 100),
                        MilestoneStatus::InProgress => egui::Color32::from_rgb(100, 150, 255),
                        MilestoneStatus::Todo => egui::Color32::GRAY,
                    };
                    ui.label(egui::RichText::new(&m.title).color(text_color));

                    if ui.small_button("🗑").clicked() {
                        to_delete = Some(m.id.clone());
                    }
                });
            }

            for updated in updates {
                if let Some(db) = &self.db {
                    let _ = db.upsert_milestone(&updated);
                }
                if let Some(list) = self.milestones.get_mut(project_id) {
                    if let Some(m) = list.iter_mut().find(|x| x.id == updated.id) {
                        m.status = updated.status;
                    }
                }
            }

            if let Some(id) = to_delete {
                if let Some(db) = &self.db {
                    let _ = db.delete_milestone(project_id, &id);
                }
                if let Some(list) = self.milestones.get_mut(project_id) {
                    list.retain(|x| x.id != id);
                }
            }
        }
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
        let milestones = self.milestones.get(project_id).cloned().unwrap_or_default();
        let user_meta = self
            .user_meta
            .get(project_id)
            .cloned()
            .unwrap_or_else(|| UserMeta {
                project_id: project_id.to_string(),
                pinned: false,
                notes: String::new(),
            });

        let proj_file = ProjectFile::from_data(&milestones, &user_meta);
        let _ = proj_file.save(Path::new(project_path));
    }
}

impl eframe::App for ProjectShelfApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_scan_results();

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
            self.render_project_details(ui);
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

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", now.as_secs(), now.subsec_nanos())
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

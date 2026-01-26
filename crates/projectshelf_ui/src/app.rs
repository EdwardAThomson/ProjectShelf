use eframe::egui;
use projectshelf_core::{config, scan_projects, Database, LanguageBreakdown, Project};
use std::collections::HashMap;
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
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
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
                    (idx, p.icon_kind.emoji().to_string(), p.name.clone(), dirty_badge.to_string(), branch_info.to_string())
                })
            })
            .collect();

        let mut new_selection = self.selected_idx;

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (idx, emoji, name, dirty, branch) in &display_items {
                    let is_selected = new_selection == Some(*idx);
                    ui.horizontal(|ui| {
                        let base_label = if branch.is_empty() {
                            format!("{} {}", emoji, name)
                        } else {
                            format!("{} {} [{}]", emoji, name, branch)
                        };
                        let response = ui.selectable_label(is_selected, &base_label);
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
            if ui.button("📂 Open Folder").clicked() {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&project.path)
                    .spawn();
            }
            if ui.button("💻 Terminal").clicked() {
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
            if ui.button("📝 Open in IDE").clicked() {
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

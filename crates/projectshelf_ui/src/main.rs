mod app;

use app::ProjectShelfApp;
use eframe::NativeOptions;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ProjectShelf",
        options,
        Box::new(|cc| Ok(Box::new(ProjectShelfApp::new(cc)))),
    )
}

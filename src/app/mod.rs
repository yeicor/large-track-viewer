pub(crate) mod cli;

use eframe::egui;

pub struct LargeTrackViewerApp {
    cli_args: cli::Cli,
}

impl LargeTrackViewerApp {
    pub fn new(cli_args: cli::Cli, _cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        Self { cli_args }
    }
}

impl eframe::App for LargeTrackViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("Large Track Viewer -- {:?}", self.cli_args));
            ui.separator();
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        // Save app state here
    }
}

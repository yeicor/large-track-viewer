pub(crate) mod settings;

use crate::app::settings::Settings;
use crate::entrypoints::profiling::profiling_ui;
use eframe::egui;

pub struct LargeTrackViewerApp {
    cli_args: settings::Settings,
}

impl LargeTrackViewerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            cli_args: Settings::from_cli(),
        }
    }
}

#[profiling::all_functions]
impl eframe::App for LargeTrackViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("Large Track Viewer -- {:?}", self.cli_args));
            ui.separator();
        });
        egui::SidePanel::right("settings_panel").show(ctx, |ui| {
            ui.heading("Settings");
            ui.separator();
            profiling_ui(ui);
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        // Save app state here
    }
}

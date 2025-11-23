//! UI panels for the application
//!
//! This module provides reusable UI components for settings, statistics,
//! file management, and other control panels.

use crate::app::state::{AppState, TilesProvider};
use egui::{Color32, RichText, Ui};

/// Render the settings panel
pub fn settings_panel(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Settings");
    ui.separator();

    ui.collapsing("Display", |ui| {
        ui.label("Track Appearance");
        ui.add_space(4.0);

        // Line width slider
        ui.horizontal(|ui| {
            ui.label("Line Width:");
            ui.add(
                egui::Slider::new(&mut state.ui_settings.line_width, 0.5..=10.0)
                    .suffix(" px")
                    .step_by(0.5),
            );
        });

        // Color picker
        ui.horizontal(|ui| {
            ui.label("Track Color:");
            ui.color_edit_button_srgba(&mut state.ui_settings.color);
        });

        ui.add_space(8.0);
    });

    ui.collapsing("Level of Detail", |ui| {
        ui.label("LOD Bias (Higher = More Detail)");
        ui.add_space(4.0);

        let mut bias = state.ui_settings.bias;
        let changed = ui
            .add(
                egui::Slider::new(&mut bias, 0.1..=10.0)
                    .logarithmic(true)
                    .step_by(0.1),
            )
            .changed();

        if changed {
            state.update_bias(bias);
        }

        ui.add_space(4.0);
        ui.label(
            RichText::new("⚠ Note: Changing bias requires reloading routes")
                .small()
                .color(ui.visuals().warn_fg_color),
        );

        ui.add_space(8.0);
    });

    ui.collapsing("Map Tiles", |ui| {
        ui.label("Select Tile Provider");
        ui.add_space(4.0);

        for provider in TilesProvider::all() {
            let selected = state.ui_settings.tiles_provider == *provider;
            if ui.selectable_label(selected, provider.name()).clicked() {
                state.ui_settings.tiles_provider = *provider;
            }
        }

        ui.add_space(4.0);
        ui.label(
            RichText::new(state.ui_settings.tiles_provider.attribution())
                .small()
                .italics(),
        );

        ui.add_space(8.0);
    });

    ui.collapsing("Debug", |ui| {
        ui.checkbox(
            &mut state.ui_settings.show_boundary_debug,
            "Show boundary context markers",
        );
        ui.label(RichText::new("Green = prev point, Red = next point").small());

        ui.add_space(8.0);
    });

    ui.separator();

    // Panel visibility toggles
    ui.collapsing("Panels", |ui| {
        ui.checkbox(&mut state.ui_settings.show_stats, "Show Statistics");
        ui.checkbox(&mut state.ui_settings.show_settings, "Show Settings");
    });
}

/// Render the statistics panel
pub fn statistics_panel(ui: &mut Ui, state: &AppState) {
    ui.heading("Statistics");
    ui.separator();

    ui.label(
        RichText::new("📊 Data Overview")
            .strong()
            .color(ui.visuals().strong_text_color()),
    );
    ui.add_space(4.0);

    // Route stats
    ui.horizontal(|ui| {
        ui.label("Routes:");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(state.stats.format_routes()).strong());
        });
    });

    ui.horizontal(|ui| {
        ui.label("Total Points:");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(state.stats.format_points()).strong());
        });
    });

    ui.horizontal(|ui| {
        ui.label("Total Distance:");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(state.stats.format_distance()).strong());
        });
    });

    ui.add_space(8.0);
    ui.separator();

    // Performance stats
    ui.label(
        RichText::new("⚡ Performance")
            .strong()
            .color(ui.visuals().strong_text_color()),
    );
    ui.add_space(4.0);

    if state.stats.last_query_time_ms > 0.0 {
        ui.horizontal(|ui| {
            ui.label("Last Query:");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = if state.stats.last_query_time_ms < 16.0 {
                    Color32::GREEN
                } else if state.stats.last_query_time_ms < 100.0 {
                    Color32::YELLOW
                } else {
                    Color32::RED
                };
                ui.label(
                    RichText::new(format!("{:.1} ms", state.stats.last_query_time_ms)).color(color),
                );
            });
        });

        ui.horizontal(|ui| {
            ui.label("Segments Rendered:");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{}", state.stats.last_query_segments)).strong());
            });
        });
    } else {
        ui.label(RichText::new("No query data yet").italics().weak());
    }

    ui.add_space(8.0);
    ui.separator();

    // Viewport info
    if let Some((min_lat, min_lon, max_lat, max_lon)) = state.stats.viewport_bounds {
        ui.label(
            RichText::new("🗺 Viewport")
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        ui.add_space(4.0);

        ui.label(format!("Lat: {:.4}° to {:.4}°", min_lat, max_lat));
        ui.label(format!("Lon: {:.4}° to {:.4}°", min_lon, max_lon));

        let width_deg = (max_lon - min_lon).abs();
        let height_deg = (max_lat - min_lat).abs();
        ui.label(format!("Size: {:.4}° × {:.4}°", width_deg, height_deg));
    }
}

/// Render the file management panel
pub fn file_management_panel(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Files");
    ui.separator();

    // Add file button
    ui.horizontal(|ui| {
        if ui.button("📂 Load GPX File...").clicked() {
            state.file_loader.show_picker = true;
        }

        if ui.button("🗑 Clear All").clicked() {
            state.clear_routes();
        }
    });

    ui.add_space(8.0);

    // Loading progress
    if state.file_loader.is_busy() {
        ui.separator();
        ui.label(
            RichText::new("⏳ Loading...")
                .strong()
                .color(ui.visuals().warn_fg_color),
        );

        if let Some(ref loading) = state.file_loader.loading_file {
            ui.label(
                RichText::new(format!(
                    "Current: {}",
                    loading.file_name().unwrap_or_default().to_string_lossy()
                ))
                .small(),
            );
        }

        let total =
            state.file_loader.loaded_files.len() + state.file_loader.pending_files.len() + 1;
        let progress = state.file_loader.progress(total);
        ui.add(egui::ProgressBar::new(progress).show_percentage());

        ui.add_space(8.0);
    }

    // Loaded files list
    if !state.file_loader.loaded_files.is_empty() {
        ui.separator();
        ui.label(
            RichText::new(format!(
                "✓ Loaded ({} files)",
                state.file_loader.loaded_files.len()
            ))
            .strong()
            .color(Color32::GREEN),
        );
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(150.0)
            .show(ui, |ui| {
                for file in &state.file_loader.loaded_files {
                    ui.label(
                        RichText::new(format!(
                            "• {}",
                            file.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .small(),
                    );
                }
            });
    }

    // Error list
    if !state.file_loader.errors.is_empty() {
        ui.separator();
        ui.label(
            RichText::new(format!(
                "⚠ Errors ({} files)",
                state.file_loader.errors.len()
            ))
            .strong()
            .color(Color32::RED),
        );
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(150.0)
            .show(ui, |ui| {
                for (file, error) in &state.file_loader.errors {
                    ui.label(
                        RichText::new(format!(
                            "• {}: {}",
                            file.file_name().unwrap_or_default().to_string_lossy(),
                            error
                        ))
                        .small()
                        .color(Color32::RED),
                    );
                }
            });

        ui.add_space(4.0);
        if ui.button("Clear Errors").clicked() {
            state.file_loader.errors.clear();
        }
    }
}

/// Render a simple file picker (native only)
#[cfg(not(target_arch = "wasm32"))]
pub fn show_file_picker(state: &mut AppState) {
    if state.file_loader.show_picker {
        state.file_loader.show_picker = false;

        if let Some(path) = rfd::FileDialog::new()
            .add_filter("GPX Files", &["gpx"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            state.queue_file(path);
        }
    }
}

/// Render a simple file picker (web version - placeholder)
#[cfg(target_arch = "wasm32")]
pub fn show_file_picker(state: &mut AppState) {
    if state.file_loader.show_picker {
        state.file_loader.show_picker = false;
        // Web file picker would require async file reading
        // This is a placeholder for web implementation
        tracing::warn!("File picker not yet implemented for web");
    }
}

/// Render the help overlay
pub fn help_overlay(ctx: &egui::Context, show: &mut bool) {
    egui::Window::new("Help")
        .open(show)
        .collapsible(false)
        .resizable(true)
        .default_width(400.0)
        .show(ctx, |ui| {
            ui.heading("Large Track Viewer");
            ui.separator();

            ui.label("A high-performance viewer for large GPS track collections.");
            ui.add_space(8.0);

            ui.label(RichText::new("🖱 Map Controls").strong());
            ui.label("• Left drag: Pan the map");
            ui.label("• Mouse wheel: Zoom in/out");
            ui.label("• Double click: Zoom in");
            ui.add_space(8.0);

            ui.label(RichText::new("📂 Loading Tracks").strong());
            ui.label("• Click 'Load GPX File' to add tracks");
            ui.label("• Multiple files can be loaded");
            ui.label("• Large files are indexed automatically");
            ui.add_space(8.0);

            ui.label(RichText::new("⚙️ Settings").strong());
            ui.label("• Adjust LOD bias for detail level");
            ui.label("• Change colors and line width");
            ui.label("• Select different map tile providers");
            ui.add_space(8.0);

            ui.label(RichText::new("⚡ Performance").strong());
            ui.label("• LOD system adapts to zoom level");
            ui.label("• Query times shown in statistics");
            ui.label("• Target: <100ms for large datasets");
            ui.add_space(8.0);

            ui.separator();
            ui.label(
                RichText::new("Press F1 to toggle this help")
                    .small()
                    .italics(),
            );
        });
}

//! Application module
//!
//! This module provides the main application structure, integrating:
//! - Map rendering via walkers
//! - GPX route data management
//! - UI panels and controls
//! - State management

mod plugin;
pub(crate) mod settings;
mod state;
mod ui_panels;

use crate::app::plugin::TrackPlugin;
use crate::app::settings::Settings;
use crate::app::state::AppState;
use eframe::egui;
use egui_eframe_entrypoints::profiling_ui;
use walkers::{HttpTiles, Map, MapMemory, sources::OpenStreetMap};

/// Main application structure
pub struct LargeTrackViewerApp {
    /// CLI arguments and initial settings
    #[allow(dead_code)] // May be used for future features
    cli_args: Settings,

    /// Application state (routes, UI settings, etc.)
    state: AppState,

    /// Map tiles provider
    tiles: HttpTiles,

    /// Map state (camera position, zoom, etc.)
    map_memory: MapMemory,

    /// Show help overlay
    show_help: bool,
}

impl LargeTrackViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cli_args = Settings::from_cli();

        // Initialize application state
        let state = AppState::new(&cli_args);

        // Create tiles provider (using OpenStreetMap for now)
        // TODO: Support dynamic tile provider selection from state.ui_settings.tiles_provider
        let tiles = HttpTiles::new(OpenStreetMap, cc.egui_ctx.clone());

        // Create map memory with default settings
        // TODO: Set initial position from cli_args if provided
        let map_memory = MapMemory::default();

        // Load initial GPX files from CLI
        // We'll process these in the update loop to avoid blocking startup
        tracing::info!(
            "Initialized with {} files to load",
            state.file_loader.pending_files.len()
        );

        Self {
            cli_args,
            state,
            tiles,
            map_memory,
            show_help: false,
        }
    }
}

#[profiling::all_functions]
impl eframe::App for LargeTrackViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        profiling::scope!("LargeTrackViewerApp::update");

        // Process pending file loads (one per frame to avoid blocking)
        if self.state.file_loader.is_busy() {
            self.state.process_pending_files();
            ctx.request_repaint(); // Keep updating while loading
        }

        // Handle keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::F1) {
                self.show_help = !self.show_help;
            }
            if i.key_pressed(egui::Key::H) && i.modifiers.ctrl {
                self.show_help = !self.show_help;
            }
        });

        // Show help overlay
        if self.show_help {
            ui_panels::help_overlay(ctx, &mut self.show_help);
        }

        // Handle file picker
        ui_panels::show_file_picker(&mut self.state);

        // Left panel: File management and settings
        egui::SidePanel::left("left_panel")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // File management section
                    ui_panels::file_management_panel(ui, &mut self.state);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // Settings section
                    if self.state.ui_settings.show_settings {
                        ui_panels::settings_panel(ui, &mut self.state);
                    }
                });
            });

        // Right panel: Statistics and profiling
        egui::SidePanel::right("right_panel")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Statistics section
                    if self.state.ui_settings.show_stats {
                        ui_panels::statistics_panel(ui, &self.state);
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);
                    }

                    // Profiling section
                    ui.heading("Profiling");
                    ui.separator();
                    profiling_ui(ui);
                });
            });

        // Top panel: Quick info and controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Large Track Viewer");
                ui.separator();

                // Quick stats
                ui.label(format!("Routes: {}", self.state.stats.route_count));
                ui.separator();
                ui.label(format!("Points: {}", self.state.stats.format_points()));
                ui.separator();
                ui.label(format!("Distance: {}", self.state.stats.format_distance()));

                // Loading indicator
                if self.state.file_loader.is_busy() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("⏳ Loading...").color(ui.visuals().warn_fg_color),
                    );
                }

                // Help button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("❓ Help (F1)").clicked() {
                        self.show_help = !self.show_help;
                    }
                });
            });
        });

        // Central panel: Map view
        egui::CentralPanel::default().show(ctx, |ui| {
            profiling::scope!("map_panel");

            // Create track rendering plugin
            let track_plugin = TrackPlugin::new(
                self.state.route_collection.clone(),
                self.state.ui_settings.color,
                self.state.ui_settings.line_width,
            )
            .with_boundary_context(!self.state.ui_settings.show_boundary_debug);

            // Measure query time
            let query_start = instant::Instant::now();

            // Render the map with plugin
            let mut map = Map::new(
                Some(&mut self.tiles),
                &mut self.map_memory,
                walkers::lat_lon(0.0, 0.0), // Default position, overridden by memory
            );
            map = map.with_plugin(track_plugin);

            // Add debug plugin if enabled
            if self.state.ui_settings.show_boundary_debug {
                let debug_plugin =
                    plugin::BoundaryContextPlugin::new(self.state.route_collection.clone());
                map = map.with_plugin(debug_plugin);
            }

            ui.add(map);

            // Update query statistics
            let query_time = query_start.elapsed();
            self.state.stats.last_query_time_ms = query_time.as_secs_f64() * 1000.0;

            // Update viewport bounds for statistics
            // TODO: Access MapMemory center position properly
            // let center = self.map_memory.center();
            // let zoom = self.map_memory.zoom();
            // self.state.stats.viewport_bounds = Some((
            //     center.lat() - lat_span / 2.0,
            //     center.lon() - lon_span / 2.0,
            //     center.lat() + lat_span / 2.0,
            //     center.lon() + lon_span / 2.0,
            // ));

            // Show attribution
            ui.painter().text(
                ui.max_rect().left_bottom() + egui::vec2(5.0, -5.0),
                egui::Align2::LEFT_BOTTOM,
                self.state.ui_settings.tiles_provider.attribution(),
                egui::FontId::proportional(10.0),
                ui.visuals().weak_text_color(),
            );
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Save UI settings
        storage.set_string("bias", self.state.ui_settings.bias.to_string());
        storage.set_string("line_width", self.state.ui_settings.line_width.to_string());

        // TODO: Save map center and zoom when MapMemory API is accessible
        // let center = self.map_memory.center();
        // storage.set_string("map_center_lat", center.lat().to_string());
        // storage.set_string("map_center_lon", center.lon().to_string());
        // storage.set_string("map_zoom", self.map_memory.zoom().to_string());
    }
}

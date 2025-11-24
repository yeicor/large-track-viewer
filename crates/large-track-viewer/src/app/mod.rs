//! Application module
//!
//! This module provides the main application structure with a clean UI:
//! - Full-screen map view
//! - Toggleable sidebar with tabs (Tracks and Settings)
//! - Drag-and-drop support for GPX files
//! - Map navigation controls for accessibility
//! - Responsive layout (sidebar from bottom on portrait displays)

mod plugin;
pub(crate) mod settings;
mod state;
mod ui_panels;

use crate::app::plugin::TrackPlugin;
use crate::app::settings::Settings;
use crate::app::state::AppState;
use eframe::egui;
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
        // Handle keyboard shortcuts and scroll detection
        ctx.input(|i| {
            if i.key_pressed(egui::Key::F1) {
                self.show_help = !self.show_help;
            }
            if i.key_pressed(egui::Key::H) && i.modifiers.ctrl {
                self.show_help = !self.show_help;
            }

            // Detect mouse wheel usage without Ctrl
            if i.raw_scroll_delta.y != 0.0 && !i.modifiers.ctrl && !self.state.show_wheel_warning {
                self.state.show_wheel_zoom_warning();
            }
        });

        // Auto-hide wheel warning after 0.5 seconds
        if self.state.show_wheel_warning && self.state.should_hide_wheel_warning() {
            self.state.hide_wheel_zoom_warning();
        }

        // Handle drag and drop
        ui_panels::handle_drag_and_drop(ctx, &mut self.state);

        // Handle file picker
        ui_panels::show_file_picker(&mut self.state);

        // Show help overlay if enabled
        if self.show_help {
            ui_panels::help_overlay(ctx, &mut self.show_help);
        }

        // Render the main sidebar (responsive: side or bottom based on orientation)
        ui_panels::render_sidebar(ctx, &mut self.state);

        // Central panel: Map view (full screen)
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE) // No frame for clean look
            .show(ctx, |ui| {
                profiling::scope!("map_panel");

                // Create track rendering plugin
                let track_plugin = TrackPlugin::new(
                    self.state.route_collection.clone(),
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

                // Render overlaid controls
                // Sidebar toggle button (top-right)
                ui_panels::sidebar_toggle_button(ui, &mut self.state);

                // Show attribution (bottom-center)
                let painter = ui.painter();
                let screen_rect = ui.max_rect();
                painter.text(
                    screen_rect.center_bottom() + egui::vec2(0.0, -5.0),
                    egui::Align2::CENTER_BOTTOM,
                    self.state.ui_settings.tiles_provider.attribution(),
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_black_alpha(180),
                );

                // Show mouse wheel zoom warning if needed
                if self.state.show_wheel_warning {
                    ui_panels::show_wheel_zoom_warning(ui, &mut self.state);
                }
            });

        // Process pending file loads at the END of the frame, after all rendering
        // This ensures no locks are held during file I/O and prevents deadlocks
        if self.state.file_loader.is_busy() {
            self.state.process_pending_files();
            ctx.request_repaint(); // Keep updating while loading
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Save UI settings
        storage.set_string("bias", self.state.ui_settings.bias.to_string());
        storage.set_string("line_width", self.state.ui_settings.line_width.to_string());
        storage.set_string(
            "sidebar_open",
            self.state.ui_settings.sidebar_open.to_string(),
        );
        storage.set_string(
            "active_tab",
            format!("{:?}", self.state.ui_settings.active_tab),
        );

        // Save tiles provider
        storage.set_string(
            "tiles_provider",
            format!("{:?}", self.state.ui_settings.tiles_provider),
        );

        // TODO: Save map center and zoom when MapMemory API is accessible
        // let center = self.map_memory.center();
        // storage.set_string("map_center_lat", center.lat().to_string());
        // storage.set_string("map_center_lon", center.lon().to_string());
        // storage.set_string("map_zoom", self.map_memory.zoom().to_string());
    }
}

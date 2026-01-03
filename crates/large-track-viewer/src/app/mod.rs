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
pub mod storage;
mod ui_panels;

use crate::app::plugin::{RenderStats, TrackPlugin};
use crate::app::settings::Settings;
use crate::app::state::{AppState, SidebarTab, TilesProvider};
use eframe::egui;
use eframe_entrypoints::async_runtime::RwLock;
use egui::DroppedFile;
use std::sync::Arc;
use walkers::{
    HttpTiles, Map, MapMemory, TileId,
    sources::{Attribution, OpenStreetMap, TileSource},
};

/// Custom OpenTopoMap tile source
pub struct OpenTopoMap;

impl TileSource for OpenTopoMap {
    fn tile_url(&self, tile_id: TileId) -> String {
        format!(
            "http://tile.opentopomap.org/{}/{}/{}.png",
            tile_id.zoom, tile_id.x, tile_id.y
        )
    }

    fn attribution(&self) -> Attribution {
        Attribution {
            text: "© OpenTopoMap (CC-BY-SA)",
            url: "https://opentopomap.org/",
            logo_light: None,
            logo_dark: None,
        }
    }

    fn max_zoom(&self) -> u8 {
        17 // OpenTopoMap has max zoom of 17
    }
}

/// Persisted settings (lightweight, no route data)
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedSettings {
    /// UI settings
    line_width: f32,
    show_outline: bool,
    bias: f64,
    sidebar_open: bool,
    active_tab: String,
    tiles_provider: String,
    show_profiling: bool,
    /// File paths that were loaded (will need to be reloaded)
    loaded_file_paths: Vec<String>,
}

/// Main application structure
pub struct LargeTrackViewerApp {
    /// Application state (routes, UI settings, etc.)
    state: AppState,

    /// Map tiles provider (OpenStreetMap)
    tiles_osm: HttpTiles,

    /// Map tiles provider (OpenTopoMap)
    tiles_otm: HttpTiles,

    /// Map state (camera position, zoom, etc.)
    map_memory: MapMemory,

    /// Show help overlay
    show_help: bool,

    /// Shared render statistics (updated by plugin each frame)
    render_stats: Arc<RwLock<RenderStats>>,

    /// Whether we've finished restoring persisted state
    restored_persisted_state: bool,

    /// Whether we've started initial parallel load
    started_initial_parallel_load: bool,
}

impl LargeTrackViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cli_args = Settings::from_cli();

        // Try to restore persisted settings (not route data)
        let mut state = if !cli_args.ignore_persisted {
            if let Some(storage) = cc.storage {
                Self::load_persisted_settings(storage, &cli_args)
            } else {
                AppState::new(&cli_args)
            }
        } else {
            tracing::info!("Ignoring persisted state (--ignore-persisted flag)");
            AppState::new(&cli_args)
        };

        // Add any CLI-specified files to pending (they take priority)
        for file_path in &cli_args.gpx_files {
            state.queue_file(DroppedFile {
                name: file_path
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                    .to_owned(),
                path: Some(file_path.clone()),
                ..Default::default()
            });
        }

        // Create tiles providers
        let tiles_osm = HttpTiles::new(OpenStreetMap, cc.egui_ctx.clone());
        let tiles_otm = HttpTiles::new(OpenTopoMap, cc.egui_ctx.clone());

        // Create map memory with default settings
        let map_memory = MapMemory::default();

        tracing::info!(
            "Initialized with {} files to load",
            state.file_loader.pending_files.len()
        );

        Self {
            state,
            tiles_osm,
            tiles_otm,
            map_memory,
            show_help: false,
            render_stats: Arc::new(RwLock::new(RenderStats::default())),
            restored_persisted_state: false,
            started_initial_parallel_load: false,
        }
    }

    /// Load persisted settings from storage (fast, no route data)
    ///
    /// Behavior:
    /// 1. Try eframe's provided `storage` first (this covers most desktop & web runner cases).
    /// 2. If not present there, try the platform backend:
    ///    - On web: use the browser localStorage backend (direct default backend function).
    ///    - On native: use the file-backed backend (returns Result<Box<dyn StorageBackend>, ...>).
    fn load_persisted_settings(storage: &dyn eframe::Storage, cli_args: &Settings) -> AppState {
        // 1) Try eframe storage first
        if let Some(json) = storage.get_string("persisted_settings")
            && !json.is_empty()
            && let Ok(settings) = serde_json::from_str::<PersistedSettings>(&json)
        {
            tracing::info!("Restored settings from eframe storage, will reload files");
            return Self::state_from_persisted_settings(settings, cli_args);
        }

        // 2) Try platform default storage backend (use free JSON helper to read structured settings)
        #[cfg(target_arch = "wasm32")]
        {
            // On web the default backend returns a concrete backend directly; use it and attempt to load JSON.
            let backend = crate::app::storage::default_storage_backend();
            match crate::app::storage::load_json_backend::<PersistedSettings>(
                backend.as_ref(),
                "persisted_settings",
            ) {
                Ok(Some(settings)) => {
                    tracing::info!(
                        "Restored settings from platform backend (web localStorage), will reload files"
                    );
                    return Self::state_from_persisted_settings(settings, cli_args);
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!("Error reading platform persisted settings (web): {:?}", e)
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // On native the default backend may return a Result (file backend). Handle initialization errors.
            match crate::app::storage::default_storage_backend() {
                Ok(backend_box) => {
                    let backend = backend_box.as_ref();
                    match crate::app::storage::load_json_backend::<PersistedSettings>(
                        backend,
                        "persisted_settings",
                    ) {
                        Ok(Some(settings)) => {
                            tracing::info!(
                                "Restored settings from platform backend (file storage), will reload files"
                            );
                            return Self::state_from_persisted_settings(settings, cli_args);
                        }
                        Ok(None) => {}
                        Err(e) => tracing::debug!(
                            "Error reading platform persisted settings (file): {:?}",
                            e
                        ),
                    }
                }
                Err(e) => tracing::debug!("Platform storage backend not available: {:?}", e),
            }
        }

        tracing::info!("No persisted settings found, starting fresh");
        AppState::new(cli_args)
    }

    /// Create AppState from persisted settings
    fn state_from_persisted_settings(settings: PersistedSettings, cli_args: &Settings) -> AppState {
        use crate::app::state::{FileLoader, UiSettings};
        use large_track_lib::{Config, RouteCollection};

        let ui_settings = UiSettings {
            line_width: settings.line_width,
            show_outline: settings.show_outline,
            bias: settings.bias,
            tiles_provider: match settings.tiles_provider.as_str() {
                "OpenTopoMap" => TilesProvider::OpenTopoMap,
                _ => TilesProvider::OpenStreetMap,
            },
            sidebar_open: settings.sidebar_open,
            active_tab: match settings.active_tab.as_str() {
                "Settings" => SidebarTab::Settings,
                _ => SidebarTab::Tracks,
            },
            show_profiling: settings.show_profiling,
        };

        // Queue files for reloading (persisted + CLI), deduplicating by canonical path
        let mut pending_files: Vec<DroppedFile> = Vec::new();
        let mut seen_paths: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();

        // Helper to add files with deduplication
        let mut add_file = |path: std::path::PathBuf| {
            if path.exists() {
                // Use canonical path to detect duplicates regardless of relative/absolute paths
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                if seen_paths.insert(canonical) {
                    pending_files.push(DroppedFile {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        path: Some(path),
                        ..Default::default()
                    });
                }
            }
        };

        // Add persisted files first
        for path_str in &settings.loaded_file_paths {
            add_file(std::path::PathBuf::from(path_str));
        }

        // Add CLI files (will be deduplicated if already in persisted)
        for path in &cli_args.gpx_files {
            add_file(path.clone());
        }

        let config = Config {
            bias: settings.bias,
            max_points_per_node: cli_args.max_points_per_node,
            reference_pixel_viewport: geo::Rect::new(
                geo::Coord { x: 0.0, y: 0.0 },
                geo::Coord {
                    x: cli_args.reference_viewport_width as f64,
                    y: cli_args.reference_viewport_height as f64,
                },
            ),
        };

        let file_loader = FileLoader {
            pending_files,
            errors: Vec::new(),
            loaded_files: Vec::new(),
            // Use a standard mutex for the results queue and an atomic counter for totals.
            // This simplifies concurrency: workers push into the mutex-protected Vec and
            // update the atomic counter; the UI thread can lock briefly to pop results.
            parallel_load_results: Arc::new(std::sync::Mutex::new(Vec::new())),
            parallel_total_files: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };

        AppState {
            route_collection: Arc::new(RwLock::new(RouteCollection::new(config))),
            ui_settings,
            file_loader,
            stats: Default::default(),
            // Initialize the shared async RwLock used for selection throughout the app.
            selected_route: Arc::new(RwLock::new(None)),
            show_wheel_warning: false,
            wheel_warning_shown_at: None,
            pending_fit_bounds: false,
            pending_reload: false,
        }
    }

    /// Fit the map view to the bounding box of all loaded tracks
    fn fit_to_bounds(&mut self) {
        // Use try_read for non-blocking UI polling.
        let collection = match self.state.route_collection.try_read() {
            Ok(guard) => guard,
            Err(_) => return, // Skip if lock is not available
        };

        if let Some((min_lat, min_lon, max_lat, max_lon)) = collection.bounding_box_wgs84() {
            let center_lat = (min_lat + max_lat) / 2.0;
            let center_lon = (min_lon + max_lon) / 2.0;

            let lat_span = (max_lat - min_lat).abs();
            let lon_span = (max_lon - min_lon).abs();
            let max_span = lat_span.max(lon_span);

            let zoom = if max_span > 0.0 {
                let zoom_estimate = (4.0 * 360.0 / max_span).log2() as f32;
                (zoom_estimate - 0.5).clamp(1.0, 18.0)
            } else {
                12.0
            };

            self.map_memory
                .center_at(walkers::lat_lon(center_lat, center_lon));
            let _ = self.map_memory.set_zoom(zoom as f64);

            tracing::trace!(
                "Auto-zoomed to bounds: ({:.4}, {:.4}) - ({:.4}, {:.4}), zoom: {:.1}",
                min_lat,
                min_lon,
                max_lat,
                max_lon,
                zoom
            );
        }
    }
}

#[profiling::all_functions]
impl eframe::App for LargeTrackViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::F1) {
                self.show_help = !self.show_help;
            }
            if i.key_pressed(egui::Key::H) && i.modifiers.ctrl {
                self.show_help = !self.show_help;
            }

            if i.raw_scroll_delta.y != 0.0 && !i.modifiers.ctrl && !self.state.show_wheel_warning {
                self.state.show_wheel_zoom_warning();
            }
        });

        // Auto-zoom to fit loaded tracks if requested
        if self.state.pending_fit_bounds {
            self.state.pending_fit_bounds = false;
            self.fit_to_bounds();
        }

        // Process pending reload (e.g., after LOD bias change)
        self.state.process_pending_reload();

        // Handle drag and drop
        ui_panels::handle_drag_and_drop(ctx, &mut self.state);

        // Handle internal file chooser
        eframe_entrypoints::file_picker::render_rust_file_dialog(ctx);
        ui_panels::manage_pending_files(&mut self.state);

        // Show help overlay if enabled
        if self.show_help {
            ui_panels::help_overlay(ctx, &mut self.show_help);
        }

        // Render the main sidebar (responsive: side or bottom based on orientation)
        ui_panels::render_sidebar(ctx, &mut self.state);

        // Capture values we need before the closure
        let route_collection = self.state.route_collection.clone();
        let line_width = self.state.ui_settings.line_width;
        let show_outline = self.state.ui_settings.show_outline;
        let tiles_provider = self.state.ui_settings.tiles_provider;
        let attribution_text = self.state.ui_settings.tiles_provider.attribution();
        let render_stats = self.render_stats.clone();

        // Central panel: Map view (full screen)
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                profiling::scope!("map_panel");

                // Use the AppState's shared `selected_route` handle directly and pass it into the plugin.
                // This centralizes selection in AppState so the plugin and the sidebar share the same lock.
                let selected_handle = self.state.selected_route.clone();
                let track_plugin = TrackPlugin::new(
                    route_collection,
                    line_width,
                    show_outline,
                    render_stats,
                    selected_handle,
                );

                let query_start = instant::Instant::now();

                let tiles: &mut HttpTiles = match tiles_provider {
                    TilesProvider::OpenStreetMap => &mut self.tiles_osm,
                    TilesProvider::OpenTopoMap => &mut self.tiles_otm,
                };

                let map = Map::new(
                    Some(tiles),
                    &mut self.map_memory,
                    walkers::lat_lon(0.0, 0.0),
                )
                .with_plugin(track_plugin);

                ui.add(map);

                // Show wheel warning and auto-hide after 0.5 seconds
                ctx.input(|i| {
                    if i.raw_scroll_delta.y != 0.0
                        && !i.modifiers.ctrl
                        && !self.state.show_wheel_warning
                    {
                        self.state.show_wheel_zoom_warning();
                    }
                });
                if self.state.show_wheel_warning && self.state.should_hide_wheel_warning() {
                    self.state.hide_wheel_zoom_warning();
                }

                let query_time = query_start.elapsed();
                self.state.stats.last_query_time_ms = query_time.as_secs_f64() * 1000.0;

                {
                    // Use try_read for non-blocking UI polling.
                    if let Ok(render_stats) = self.render_stats.try_read() {
                        self.state.stats.last_query_segments = render_stats.segments_rendered;
                        self.state.stats.last_query_simplified_points =
                            render_stats.simplified_points_rendered;
                    }
                }

                ui_panels::sidebar_toggle_button(ui, &mut self.state);

                let painter = ui.painter();
                let screen_rect = ui.max_rect();
                painter.text(
                    screen_rect.center_bottom() + egui::vec2(0.0, -5.0),
                    egui::Align2::CENTER_BOTTOM,
                    attribution_text,
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_black_alpha(180),
                );

                if self.state.show_wheel_warning {
                    ui_panels::show_wheel_zoom_warning(ui, &mut self.state);
                }
            });

        // Start parallel loading if we have pending files and haven't started yet
        if !self.started_initial_parallel_load && !self.state.file_loader.pending_files.is_empty() {
            self.started_initial_parallel_load = true;
            self.state.start_parallel_load();
            ctx.request_repaint();
        }

        // Process parallel load results (one at a time for UI responsiveness)
        let has_more_results = self.state.process_parallel_results();
        if self.state.is_parallel_loading() || has_more_results {
            // Request immediate repaint to process next result quickly while still yielding to UI
            ctx.request_repaint();
        }

        // Process any remaining files one at a time (fallback or WASM)
        if self.state.file_loader.is_busy() && !self.state.is_parallel_loading() {
            self.state.process_pending_files();
            ctx.request_repaint();
        }

        // After all persisted files are loaded, fit to bounds once
        if !self.restored_persisted_state
            && !self.state.file_loader.is_busy()
            && !self.state.file_loader.loaded_files.is_empty()
        {
            self.restored_persisted_state = true;
            self.fit_to_bounds();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Save settings only (no route data - fast)
        // Include ONLY real filesystem paths (skip synthetic web:// identifiers).
        // We intentionally do NOT persist browser-only dropped files (which are identified
        // by the synthetic web:// prefix) because they are not reloadable from disk.
        let mut all_file_paths: Vec<String> = self
            .state
            .file_loader
            .loaded_files
            .iter()
            .map(|(path, _, _)| path.to_string_lossy().to_string())
            // Filter out synthetic web-only paths (we use "web://" prefix for those)
            .filter(|s| !s.starts_with("web://"))
            .collect();

        // Add pending files (only persist those with a real filesystem path)
        for path in &self.state.file_loader.pending_files {
            if let Some(p) = path.path.as_ref() {
                let path_str = p.to_string_lossy().to_string();
                if !all_file_paths.contains(&path_str) {
                    all_file_paths.push(path_str);
                }
            } else {
                // Skip browser-dropped files without a real path (do not persist)
            }
        }

        // Add files being processed in parallel (from results queue)
        {
            // Use the mutex-based results container to read any in-progress results.
            // Locking here is brief and deterministic; on native this is a std::sync::Mutex
            // and on wasm it is likewise safe because we only hold the lock very briefly.
            let guard = self
                .state
                .file_loader
                .parallel_load_results
                .lock()
                .expect("failed to acquire lock on parallel_load_results mutex in save()");
            for (path, _) in guard.iter() {
                let path_str: String = path.to_string_lossy().to_string();
                // Skip synthetic web-only identifiers
                if path_str.starts_with("web://") {
                    continue;
                }
                if !all_file_paths.contains(&path_str) {
                    all_file_paths.push(path_str);
                }
            }
        }

        let loaded_file_paths: Vec<String> = all_file_paths
            .into_iter()
            .filter(|p| !p.starts_with("web://"))
            .collect();

        let settings = PersistedSettings {
            line_width: self.state.ui_settings.line_width,
            show_outline: self.state.ui_settings.show_outline,
            bias: self.state.ui_settings.bias,
            sidebar_open: self.state.ui_settings.sidebar_open,
            active_tab: format!("{:?}", self.state.ui_settings.active_tab),
            tiles_provider: format!("{:?}", self.state.ui_settings.tiles_provider),
            show_profiling: self.state.ui_settings.show_profiling,
            loaded_file_paths,
        };

        // Serialize settings once and persist to both eframe storage and the platform backend.
        if let Ok(json) = serde_json::to_string(&settings) {
            // Persist to eframe storage (existing behavior)
            storage.set_string("persisted_settings", json.clone());
            tracing::debug!("Saved settings to eframe storage on exit");

            // Persist to platform-specific default storage backend using the object-safe helpers.
            // On web: default_storage_backend() returns a concrete backend (Box<dyn StorageBackend>).
            // On native: default_storage_backend() returns Result<Box<dyn StorageBackend>, StorageError>.
            #[cfg(target_arch = "wasm32")]
            {
                let backend = crate::app::storage::default_storage_backend();
                match crate::app::storage::save_json_backend(
                    backend.as_ref(),
                    "persisted_settings",
                    &settings,
                ) {
                    Ok(()) => tracing::debug!("Saved settings to web storage (localStorage)"),
                    Err(e) => tracing::warn!("Failed to save settings to web storage: {:?}", e),
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                match crate::app::storage::default_storage_backend() {
                    Ok(backend_box) => {
                        let backend = backend_box.as_ref();
                        match crate::app::storage::save_json_backend(
                            backend,
                            "persisted_settings",
                            &settings,
                        ) {
                            Ok(()) => tracing::debug!("Saved settings to file storage"),
                            Err(e) => {
                                tracing::warn!("Failed to save settings to file storage: {:?}", e)
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to initialize file storage backend: {:?}", e);
                    }
                }
            }
        }
    }
}

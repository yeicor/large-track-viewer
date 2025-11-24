//! Application state management
//!
//! This module manages the application state including route collections,
//! UI settings, and file loading operations.

use crate::app::settings::Settings;
use egui::Color32;
use large_track_lib::{Config, RouteCollection};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Main application state
pub struct AppState {
    /// Route collection with all loaded tracks
    pub route_collection: Arc<RwLock<RouteCollection>>,

    /// Current UI settings
    pub ui_settings: UiSettings,

    /// File loading state
    pub file_loader: FileLoader,

    /// Statistics about loaded data
    pub stats: Stats,

    /// Whether to show the mouse wheel zoom warning
    pub show_wheel_warning: bool,

    /// Timestamp when the warning was last shown
    pub wheel_warning_shown_at: Option<instant::Instant>,
}

/// UI-specific settings that can be adjusted at runtime
#[derive(Clone)]
pub struct UiSettings {
    /// LOD bias (higher = more detail)
    pub bias: f64,

    /// Track line width in pixels
    pub line_width: f32,

    /// Show boundary context debug markers
    pub show_boundary_debug: bool,

    /// Map tiles provider
    pub tiles_provider: TilesProvider,

    /// Whether sidebar is open
    pub sidebar_open: bool,

    /// Current active tab in sidebar
    pub active_tab: SidebarTab,

    /// Whether to show profiling in settings
    pub show_profiling: bool,
}

/// Sidebar tabs
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SidebarTab {
    Tracks,
    Settings,
}

/// Available map tile providers
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TilesProvider {
    OpenStreetMap,
    OpenTopoMap,
    CyclOSM,
}

impl TilesProvider {
    pub fn url(&self) -> &'static str {
        match self {
            Self::OpenStreetMap => "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
            Self::OpenTopoMap => "https://tile.opentopomap.org/{z}/{x}/{y}.png",
            Self::CyclOSM => "https://tile.thunderforest.com/cycle/{z}/{x}/{y}.png",
        }
    }

    pub fn attribution(&self) -> &'static str {
        match self {
            Self::OpenStreetMap => "© OpenStreetMap contributors",
            Self::OpenTopoMap => "© OpenTopoMap (CC-BY-SA)",
            Self::CyclOSM => "© CyclOSM & Thunderforest",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::OpenStreetMap, Self::OpenTopoMap, Self::CyclOSM]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenStreetMap => "OpenStreetMap",
            Self::OpenTopoMap => "OpenTopoMap",
            Self::CyclOSM => "CyclOSM",
        }
    }
}

/// File loading state and operations
pub struct FileLoader {
    /// Files pending load
    pub pending_files: Vec<PathBuf>,

    /// Currently loading file
    pub loading_file: Option<PathBuf>,

    /// Load errors
    pub errors: Vec<(PathBuf, String)>,

    /// Successfully loaded files with their GPX data for potential removal
    pub loaded_files: Vec<(PathBuf, gpx::Gpx)>,

    /// Show file picker dialog
    pub show_picker: bool,
}

/// Statistics about loaded data
#[derive(Default)]
pub struct Stats {
    /// Total number of routes
    pub route_count: usize,

    /// Total number of track points
    pub total_points: usize,

    /// Total distance in meters
    pub total_distance: f64,

    /// Last query time in milliseconds
    pub last_query_time_ms: f64,

    /// Number of segments in last query
    pub last_query_segments: usize,

    /// Current viewport bounds (lat/lon)
    pub viewport_bounds: Option<(f64, f64, f64, f64)>, // (min_lat, min_lon, max_lat, max_lon)
}

impl AppState {
    /// Create new application state from CLI settings
    pub fn new(settings: &Settings) -> Self {
        let config = Config {
            bias: settings.bias,
            max_points_per_node: settings.max_points_per_node,
            reference_pixel_viewport: geo::Rect::new(
                geo::Coord { x: 0.0, y: 0.0 },
                geo::Coord {
                    x: settings.reference_viewport_width as f64,
                    y: settings.reference_viewport_height as f64,
                },
            ),
        };

        let route_collection = Arc::new(RwLock::new(RouteCollection::new(config)));

        let ui_settings = UiSettings {
            bias: settings.bias,
            line_width: settings.line_width,
            show_boundary_debug: false,
            tiles_provider: TilesProvider::OpenStreetMap,
            sidebar_open: true,
            active_tab: SidebarTab::Tracks,
            show_profiling: false,
        };

        let file_loader = FileLoader {
            pending_files: settings.gpx_files.clone(),
            loading_file: None,
            errors: Vec::new(),
            loaded_files: Vec::new(),
            show_picker: false,
        };

        Self {
            route_collection,
            ui_settings,
            file_loader,
            stats: Stats::default(),
            show_wheel_warning: false,
            wheel_warning_shown_at: None,
        }
    }

    /// Load a GPX file into the collection
    pub fn load_gpx_file(&mut self, path: PathBuf) -> Result<(), String> {
        profiling::scope!("load_gpx_file");

        self.file_loader.loading_file = Some(path.clone());

        // Read and parse the GPX file OUTSIDE of the lock
        let gpx_result = (|| -> Result<gpx::Gpx, String> {
            let file =
                std::fs::File::open(&path).map_err(|e| format!("Failed to open file: {}", e))?;
            let reader = std::io::BufReader::new(file);
            gpx::read(reader).map_err(|e| format!("Failed to parse GPX: {}", e))
        })();

        self.file_loader.loading_file = None;

        match gpx_result {
            Ok(gpx) => {
                // Only acquire the write lock when modifying the collection
                let add_result = {
                    let mut collection = self.route_collection.write().unwrap();
                    collection
                        .add_route(gpx.clone())
                        .map_err(|e| format!("Failed to add route: {}", e))
                };

                match add_result {
                    Ok(_) => {
                        self.file_loader.loaded_files.push((path.clone(), gpx));
                        self.update_stats();
                        Ok(())
                    }
                    Err(e) => {
                        self.file_loader.errors.push((path, e.clone()));
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.file_loader.errors.push((path, e.clone()));
                Err(e)
            }
        }
    }

    /// Process pending file loads
    pub fn process_pending_files(&mut self) {
        if let Some(path) = self.file_loader.pending_files.pop() {
            let _ = self.load_gpx_file(path);
        }
    }

    /// Add a file to the pending load queue
    pub fn queue_file(&mut self, path: PathBuf) {
        let already_loaded = self
            .file_loader
            .loaded_files
            .iter()
            .any(|(p, _)| p == &path);
        if !self.file_loader.pending_files.contains(&path) && !already_loaded {
            self.file_loader.pending_files.push(path);
        }
    }

    /// Remove a loaded file by index
    pub fn remove_file(&mut self, index: usize) {
        if index < self.file_loader.loaded_files.len() {
            self.file_loader.loaded_files.remove(index);
            self.rebuild_collection();
            self.update_stats();
        }
    }

    /// Rebuild the entire collection from loaded files
    fn rebuild_collection(&mut self) {
        profiling::scope!("rebuild_collection");

        // Create new collection with same config
        let config = self.route_collection.read().unwrap().config().clone();
        let mut new_collection = RouteCollection::new(config);

        // Re-add all routes
        for (_, gpx) in &self.file_loader.loaded_files {
            let _ = new_collection.add_route(gpx.clone());
        }

        // Replace the collection
        self.route_collection = Arc::new(RwLock::new(new_collection));
    }

    /// Update statistics from the route collection
    pub fn update_stats(&mut self) {
        profiling::scope!("update_stats");

        let collection = self.route_collection.read().unwrap();
        let info = collection.get_info();

        self.stats.route_count = info.route_count;
        self.stats.total_points = info.total_points;
        self.stats.total_distance = info.total_distance_meters;
    }

    /// Clear all loaded routes
    pub fn clear_routes(&mut self) {
        let config = self.route_collection.read().unwrap().config().clone();
        self.route_collection = Arc::new(RwLock::new(RouteCollection::new(config)));
        self.file_loader.loaded_files.clear();
        self.file_loader.errors.clear();
        self.file_loader.pending_files.clear();
        self.stats = Stats::default();
    }

    /// Update LOD bias in the collection
    pub fn update_bias(&mut self, new_bias: f64) {
        self.ui_settings.bias = new_bias;
        // Note: Bias change requires rebuilding the quadtree
        // For now, we just update the UI setting
        // A production implementation would rebuild or support dynamic bias
    }

    /// Show the mouse wheel zoom warning
    pub fn show_wheel_zoom_warning(&mut self) {
        self.show_wheel_warning = true;
        self.wheel_warning_shown_at = Some(instant::Instant::now());
    }

    /// Hide the mouse wheel zoom warning
    pub fn hide_wheel_zoom_warning(&mut self) {
        self.show_wheel_warning = false;
    }

    /// Check if the warning should auto-hide (after 0.5 seconds)
    pub fn should_hide_wheel_warning(&self) -> bool {
        if let Some(shown_at) = self.wheel_warning_shown_at {
            shown_at.elapsed().as_secs_f32() >= 0.5
        } else {
            false
        }
    }

    /// Get fade alpha for the wheel warning (0.0 to 1.0)
    /// Fade in over 0.15s, stay visible, fade out over 0.15s
    pub fn get_wheel_warning_alpha(&self) -> f32 {
        if let Some(shown_at) = self.wheel_warning_shown_at {
            let elapsed = shown_at.elapsed().as_secs_f32();

            if elapsed < 0.15 {
                // Fade in
                elapsed / 0.15
            } else if elapsed < 0.35 {
                // Fully visible
                1.0
            } else if elapsed < 0.5 {
                // Fade out
                1.0 - ((elapsed - 0.35) / 0.15)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Generate a color for a track based on its index
    pub fn get_track_color(index: usize) -> Color32 {
        // Generate distinct colors using HSV color space
        let hue = (index as f32 * 137.508) % 360.0; // Golden angle for better distribution
        let saturation = 0.7;
        let value = 0.9;

        // Convert HSV to RGB
        let c = value * saturation;
        let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
        let m = value - c;

        let (r, g, b) = if hue < 60.0 {
            (c, x, 0.0)
        } else if hue < 120.0 {
            (x, c, 0.0)
        } else if hue < 180.0 {
            (0.0, c, x)
        } else if hue < 240.0 {
            (0.0, x, c)
        } else if hue < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Color32::from_rgb(
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
        )
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            bias: 1.0,
            line_width: 2.0,
            show_boundary_debug: false,
            tiles_provider: TilesProvider::OpenStreetMap,
            sidebar_open: true,
            active_tab: SidebarTab::Tracks,
            show_profiling: false,
        }
    }
}

impl FileLoader {
    /// Check if any files are being processed
    pub fn is_busy(&self) -> bool {
        self.loading_file.is_some() || !self.pending_files.is_empty()
    }

    /// Get load progress (0.0 to 1.0)
    pub fn progress(&self, total_files: usize) -> f32 {
        if total_files == 0 {
            return 1.0;
        }
        let remaining = self.pending_files.len() + if self.loading_file.is_some() { 1 } else { 0 };
        1.0 - (remaining as f32 / total_files as f32)
    }
}

impl Stats {
    /// Format distance as human-readable string
    pub fn format_distance(&self) -> String {
        let km = self.total_distance / 1000.0;
        if km < 1.0 {
            format!("{:.0} m", self.total_distance)
        } else if km < 100.0 {
            format!("{:.2} km", km)
        } else {
            format!("{:.0} km", km)
        }
    }

    /// Format point count with thousands separators
    pub fn format_points(&self) -> String {
        format_number_with_commas(self.total_points)
    }

    /// Format route count
    pub fn format_routes(&self) -> String {
        format!("{}", self.route_count)
    }
}

/// Helper to format numbers with comma separators
fn format_number_with_commas(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

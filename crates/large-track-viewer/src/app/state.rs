//! Application state management
//!
//! This module manages the application state including route collections,
//! UI settings, and file loading operations.

use crate::app::settings::Settings;
use large_track_lib::{Config, RouteCollection};
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(target_arch = "wasm32")]
use std::sync::RwLock;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::RwLock;

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

    /// Whether we need to fit the map to the loaded tracks' bounds
    pub pending_fit_bounds: bool,

    /// Whether we need to reload routes due to config change
    pub pending_reload: bool,
}

/// UI-specific settings that can be adjusted at runtime
#[derive(Clone)]
pub struct UiSettings {
    /// Track line width in pixels
    pub line_width: f32,

    /// Show outline/border around tracks
    pub show_outline: bool,

    /// LOD bias (higher = more detail)
    pub bias: f64,

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
}

impl TilesProvider {
    pub fn attribution(&self) -> &'static str {
        match self {
            Self::OpenStreetMap => "© OpenStreetMap contributors",
            Self::OpenTopoMap => "© OpenTopoMap (CC-BY-SA)",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::OpenStreetMap, Self::OpenTopoMap]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenStreetMap => "OpenStreetMap",
            Self::OpenTopoMap => "OpenTopoMap",
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

    /// Results from parallel loading (path, result) - accumulated incrementally
    #[allow(clippy::type_complexity)]
    #[cfg(not(target_arch = "wasm32"))]
    pub parallel_load_results: Arc<tokio::sync::RwLock<Vec<(PathBuf, Result<gpx::Gpx, String>)>>>,
    #[cfg(target_arch = "wasm32")]
    pub parallel_load_results: Arc<std::sync::RwLock<Vec<(PathBuf, Result<gpx::Gpx, String>)>>>,

    /// Total number of files in current parallel load batch
    #[cfg(not(target_arch = "wasm32"))]
    pub parallel_total_files: Arc<tokio::sync::RwLock<usize>>,
    #[cfg(target_arch = "wasm32")]
    pub parallel_total_files: Arc<std::sync::RwLock<usize>>,
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

    /// Number of simplified points in last query (actually rendered)
    pub last_query_simplified_points: usize,
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
            line_width: settings.line_width,
            show_outline: settings.show_outline,
            bias: settings.bias,
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
            parallel_load_results: Arc::new(RwLock::new(Vec::new())),
            parallel_total_files: Arc::new(RwLock::new(0)),
        };

        Self {
            route_collection,
            ui_settings,
            file_loader,
            stats: Stats::default(),
            show_wheel_warning: false,
            wheel_warning_shown_at: None,
            pending_fit_bounds: false,
            pending_reload: false,
        }
    }

    /// Load a GPX file into the collection (single-threaded, for sequential loading)
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
                    if let Ok(mut collection) = self.route_collection.try_write() {
                        collection
                            .add_route(gpx.clone())
                            .map_err(|e| format!("Failed to add route: {}", e))
                    } else {
                        Err("Could not acquire write lock on route_collection".to_string())
                    }
                };

                match add_result {
                    Ok(_) => {
                        self.file_loader.loaded_files.push((path.clone(), gpx));
                        self.update_stats();
                        // Request auto-zoom to fit new tracks
                        self.pending_fit_bounds = true;
                        Ok(())
                    }
                    Err(e) => {
                        self.file_loader.errors.push((path, e.clone().to_string()));
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.file_loader.errors.push((path, e.clone().to_string()));
                Err(e)
            }
        }
    }

    /// Start parallel loading of all pending files
    pub fn start_parallel_load(&mut self) {
        let files_to_load: Vec<PathBuf> = self.file_loader.pending_files.drain(..).collect();
        if files_to_load.is_empty() {
            return;
        }

        let results = self.file_loader.parallel_load_results.clone();
        let total_files = self.file_loader.parallel_total_files.clone();

        // Set the totals and reset counters
        let files_len = files_to_load.len();
        let rt = tokio::runtime::Handle::try_current()
            .expect("Must be called from within a tokio runtime");

        {
            if let Ok(mut total_files_lock) = total_files.try_write() {
                *total_files_lock = files_len;
            } else {
                // Could not acquire lock; skip updating this frame
            }
        }
        // Limit concurrency to number of logical CPU cores (native only)
        let max_concurrent = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4); // fallback to 4 if detection fails
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent));

        for path in files_to_load {
            let results = results.clone();
            let semaphore = semaphore.clone();
            rt.spawn(async move {
                let permit = semaphore.acquire_owned().await.unwrap();
                use tokio::io::AsyncReadExt;
                let result = (|| async {
                    let file = tokio::fs::File::open(&path)
                        .await
                        .map_err(|e| format!("Failed to open file: {}", e))?;
                    let mut reader = tokio::io::BufReader::new(file);
                    let mut buf = Vec::new();
                    reader
                        .read_to_end(&mut buf)
                        .await
                        .map_err(|e| format!("Failed to read file: {}", e))?;
                    let cursor = std::io::Cursor::new(buf);
                    gpx::read(cursor).map_err(|e| format!("Failed to parse GPX: {}", e))
                })()
                .await;

                // Push result immediately so UI can process it while we parse the next file
                results.write().await.push((path, result));
                drop(permit); // release semaphore
                // Yield to allow other tasks to run (helps UI responsiveness)
                tokio::task::yield_now().await;
            });
        }
    }

    /// Process results from parallel loading incrementally.
    /// Processes one result per call to keep UI responsive during indexing.
    /// Returns true if there are more results to process.
    pub fn process_parallel_results(&mut self) -> bool {
        // Check if we're done (all added)
        let total = match self.file_loader.parallel_total_files.try_read() {
            Ok(guard) => *guard,
            Err(_) => 0,
        };

        let added = self.file_loader.loaded_files.len() + self.file_loader.errors.len();
        if total > 0 && added >= total {
            self.reset_parallel_loading();
            return false;
        }

        // Process exactly one result per frame to keep UI fluid during indexing
        // Take one result (non-blocking)
        let result: Option<(PathBuf, Result<gpx::Gpx, String>)> = {
            if let Ok(mut results_lock) = self.file_loader.parallel_load_results.try_write() {
                if results_lock.is_empty() {
                    None
                } else {
                    Some(results_lock.remove(0))
                }
            } else {
                None
            }
        };

        let Some((path, parse_result)) = result else {
            // No results ready yet, but we're still loading
            return self.is_parallel_loading();
        };

        match parse_result {
            Ok(gpx) => {
                // Add this single route to the collection
                let add_result = {
                    if let Ok(mut collection) = self.route_collection.try_write() {
                        collection.add_route(gpx.clone())
                    } else {
                        Err(large_track_lib::DataError::InvalidGeometry(
                            "Could not acquire write lock on route_collection".to_string(),
                        ))
                    }
                };

                match add_result {
                    Ok(_) => {
                        self.file_loader.loaded_files.push((path, gpx));
                        self.update_stats();
                        self.pending_fit_bounds = true;
                    }
                    Err(e) => {
                        self.file_loader
                            .errors
                            .push((path, format!("Failed to add route: {}", e)));
                    }
                }
                // No need to increment a processed counter; progress is now based on loaded_files + errors.
            }
            Err(e) => {
                self.file_loader.errors.push((path, e));
                // No need to increment a processed counter; progress is now based on loaded_files + errors.
            }
        }

        // Return true if there are more results to process or still loading
        let more_results = match self.file_loader.parallel_load_results.try_read() {
            Ok(results_lock) => !results_lock.is_empty(),
            Err(_) => false,
        };
        more_results || self.is_parallel_loading()
    }

    /// Check if parallel loading is in progress
    pub fn is_parallel_loading(&self) -> bool {
        let total = match self.file_loader.parallel_total_files.try_read() {
            Ok(guard) => *guard,
            Err(_) => 0,
        };
        let processed = self.file_loader.loaded_files.len() + self.file_loader.errors.len();
        total > 0 && processed < total
    }

    /// Reset parallel loading state (called when all routes are added)
    fn reset_parallel_loading(&mut self) {
        if let Ok(mut total_files_lock) = self.file_loader.parallel_total_files.try_write() {
            *total_files_lock = 0;
        }
        // WASM-specific code removed; async version is now used everywhere.
    }

    /// Get loading progress (0.0 to 1.0)
    pub fn loading_progress(&self) -> f32 {
        let total = match self.file_loader.parallel_total_files.try_read() {
            Ok(guard) => *guard,
            Err(_) => 0,
        };
        let processed = self.file_loader.loaded_files.len() + self.file_loader.errors.len();
        if total == 0 {
            0.0
        } else {
            processed as f32 / total as f32
        }
    }

    /// Get loading status text
    pub fn loading_status(&self) -> String {
        let total = match self.file_loader.parallel_total_files.try_read() {
            Ok(guard) => *guard,
            Err(_) => 0,
        };
        let processed = self.file_loader.loaded_files.len() + self.file_loader.errors.len();
        if processed < total {
            format!("{}/{}", processed, total)
        } else {
            format!("{}/{} done", processed, total)
        }
    }

    /// Process pending file loads (one at a time, for incremental loading)
    pub fn process_pending_files(&mut self) {
        // First check for parallel results
        self.process_parallel_results();

        // Then process any remaining files one at a time
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
        self.rebuild_collection_with_bias(self.ui_settings.bias);
    }

    /// Rebuild the collection with a specific bias value
    fn rebuild_collection_with_bias(&mut self, bias: f64) {
        profiling::scope!("rebuild_collection");

        // Create new collection with updated bias
        let old_config = match self.route_collection.try_read() {
            Ok(guard) => guard.config().clone(),
            Err(_) => return, // Skip if lock is not available
        };
        let config = Config { bias, ..old_config };
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

        if let Ok(collection) = self.route_collection.try_read() {
            let info = collection.get_info();

            self.stats.route_count = info.route_count;
            self.stats.total_points = info.total_points;
            self.stats.total_distance = info.total_distance_meters;
        }
    }

    /// Clear all loaded routes
    pub fn clear_routes(&mut self) {
        let config = match self.route_collection.try_read() {
            Ok(guard) => guard.config().clone(),
            Err(_) => return, // Skip if lock is not available
        };
        self.route_collection = Arc::new(RwLock::new(RouteCollection::new(config)));
        self.file_loader.loaded_files.clear();
        self.file_loader.errors.clear();
        self.file_loader.pending_files.clear();
        self.stats = Stats::default();
    }

    /// Update LOD bias and trigger reload
    pub fn update_bias(&mut self, new_bias: f64) {
        if (self.ui_settings.bias - new_bias).abs() > 0.01 {
            self.ui_settings.bias = new_bias;
            self.pending_reload = true;
        }
    }

    /// Process pending reload if needed
    pub fn process_pending_reload(&mut self) {
        if self.pending_reload {
            self.pending_reload = false;
            self.rebuild_collection_with_bias(self.ui_settings.bias);
            self.update_stats();
        }
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
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            line_width: 1.0,
            show_outline: false,
            bias: 1.0,
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
        let total = match self.parallel_total_files.try_read() {
            Ok(guard) => *guard,
            Err(_) => 0,
        };
        let processed = self.loaded_files.len() + self.errors.len();
        self.loading_file.is_some()
            || !self.pending_files.is_empty()
            || (total > 0 && processed < total)
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

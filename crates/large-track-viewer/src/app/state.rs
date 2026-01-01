//! Application state management
//!
//! This module manages the application state including route collections,
//! UI settings, and file loading operations.

use crate::app::settings::Settings;
use eframe_entrypoints::async_runtime;
use eframe_entrypoints::async_runtime::RwLock;
use egui::DroppedFile;
use large_track_lib::{Config, RouteCollection};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Generate a stable synthetic path for a dropped file when a real path is unavailable.
/// Web-dropped files often don't include a filesystem path. We use a "web://" prefix
/// combined with a unique atomic counter and the file size (when available) to
/// produce a unique identifier for web-only dropped files. If a real filesystem
/// path is present, it is returned unchanged.
fn synthetic_path_for(dropped: &DroppedFile) -> PathBuf {
    if let Some(p) = dropped.path.as_ref() {
        p.clone()
    } else {
        // Keep the counter static inside this function to avoid requiring a top-level import.
        static SYNTHETIC_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let id = SYNTHETIC_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let size = dropped.bytes.as_ref().map(|b| b.len()).unwrap_or(0);
        // Produce a URI-like synthetic identifier: web://<id>-<size>-<name>
        PathBuf::from(format!("web://{}-{}-{}", id, size, dropped.name))
    }
}

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

    /// Currently selected route (index into the collection) for highlighting/overlay.
    /// Shared across UI and plugin so both can read/write selection using an async RwLock.
    /// `None` means no route is selected.
    pub selected_route: Arc<RwLock<Option<usize>>>,

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
    pub pending_files: Vec<DroppedFile>,

    /// Load errors
    pub errors: Vec<(PathBuf, String)>,

    /// Successfully loaded files with their GPX data and the starting route index
    /// within the collection where routes from this file begin. This allows mapping
    /// loaded files to route indices later (for selection & highlighting).
    pub loaded_files: Vec<(PathBuf, gpx::Gpx, usize)>,

    /// Show file picker dialog
    pub show_picker: bool,

    /// Results from parallel loading (path, result) - accumulated incrementally
    #[allow(clippy::type_complexity)]
    pub parallel_load_results: Arc<Mutex<Vec<(PathBuf, Result<gpx::Gpx, String>)>>>,

    /// Total number of files in current parallel load batch
    pub parallel_total_files: Arc<AtomicUsize>,
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
            pending_files: settings
                .gpx_files
                .iter()
                .map(|path| DroppedFile {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: Some(path.clone()),
                    ..Default::default()
                })
                .collect(),
            errors: Vec::new(),
            loaded_files: Vec::new(),
            show_picker: false,
            parallel_load_results: Arc::new(Mutex::new(Vec::new())),
            parallel_total_files: Arc::new(AtomicUsize::new(0)),
        };

        Self {
            route_collection,
            ui_settings,
            file_loader,
            stats: Stats::default(),
            selected_route: Arc::new(RwLock::new(None)),
            show_wheel_warning: false,
            wheel_warning_shown_at: None,
            pending_fit_bounds: false,
            pending_reload: false,
        }
    }

    // Load a single file
    async fn load_file_to_gpx(file: &DroppedFile) -> Result<gpx::Gpx, String> {
        let buf = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                use tokio::io::AsyncReadExt;
                let file = tokio::fs::File::open(file.path.as_ref().unwrap())
                    .await
                    .map_err(|e| format!("Error opening file: {:?}", e))?;
                let mut reader = tokio::io::BufReader::new(file);
                let mut buf = Vec::new();
                reader
                    .read_to_end(&mut buf)
                    .await
                    .map_err(|e| format!("Failed to read file: {}", e))?;
                buf
            }
            #[cfg(target_arch = "wasm32")]
            {
                // On wasm, ensure bytes are present; if not, return an error instead of panicking.
                file.bytes
                    .clone()
                    .ok_or_else(|| "No file bytes available for dropped file".to_string())?
            }
        };
        let cursor = std::io::Cursor::new(buf);
        gpx::read(cursor).map_err(|e| format!("Failed to parse GPX: {}", e))
    }

    /// Start parallel loading of all pending files
    pub fn start_parallel_load(&mut self) {
        let files_to_load: Vec<DroppedFile> = self.file_loader.pending_files.drain(..).collect();
        if files_to_load.is_empty() {
            return;
        }

        let results = self.file_loader.parallel_load_results.clone();
        let total_files = self.file_loader.parallel_total_files.clone();

        // Set the totals and reset counters
        let files_len = files_to_load.len();

        // Store total files atomically (works on all platforms).
        // `total_files` is an `Arc<AtomicUsize>`, so we can update it directly.
        total_files.store(files_len, Ordering::SeqCst);
        // Limit concurrency to number of logical CPU cores (native only)
        // On web, we use a smaller number since web workers have more overhead
        #[cfg(not(target_arch = "wasm32"))]
        let max_concurrent = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4); // fallback to 4 if detection fails
        #[cfg(target_arch = "wasm32")]
        let max_concurrent = 4; // Use a reasonable default for web workers

        let semaphore = std::sync::Arc::new(async_runtime::Semaphore::new(max_concurrent));

        for dropped_file in files_to_load {
            let results = results.clone();
            let semaphore = semaphore.clone();
            // Use async_runtime::spawn which works on both native (tokio) and web (tokio-with-wasm)
            async_runtime::spawn(async move {
                let permit = semaphore.acquire_owned().await.unwrap();
                let result = Self::load_file_to_gpx(&dropped_file).await;
                {
                    // Compute a stable identifier for this file (real path when available,
                    // synthetic web://<name> otherwise).
                    let path = synthetic_path_for(&dropped_file);
                    let mut guard = results.lock().unwrap();
                    guard.push((path, result));
                }
                drop(permit); // release semaphore
                // Yield to allow other tasks to run (helps UI responsiveness)
                async_runtime::yield_now().await;
            });
        }
    }

    /// Process results from parallel loading incrementally.
    /// Processes one result per call to keep UI responsive during indexing.
    /// Returns true if there are more results to process.
    pub fn process_parallel_results(&mut self) -> bool {
        // Check if we're done (all added)
        let total = self.file_loader.parallel_total_files.load(Ordering::SeqCst);

        let added = self.file_loader.loaded_files.len() + self.file_loader.errors.len();
        if total > 0 && added >= total {
            self.reset_parallel_loading();
            return false;
        }

        // Process exactly one result per frame to keep UI fluid during indexing
        // Take one result (non-blocking)
        let result: Option<(PathBuf, Result<gpx::Gpx, String>)> = {
            let mut guard = self.file_loader.parallel_load_results.lock().unwrap();
            if guard.is_empty() {
                None
            } else {
                Some(guard.remove(0))
            }
        };

        let Some((path, parse_result)) = result else {
            // No results ready yet, but we're still loading
            return self.is_parallel_loading();
        };

        match parse_result {
            Ok(gpx) => {
                // Add this single route to the collection and record the starting index
                let mut start_idx_opt: Option<usize> = None;
                let add_result = {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let mut res_opt = Err(large_track_lib::DataError::InvalidGeometry(
                            "Could not acquire write lock on route_collection".to_string(),
                        ));
                        async_runtime::blocking_write(&self.route_collection, |collection| {
                            // The route will be appended; record the index where it will be inserted.
                            let start_idx = collection.route_count();
                            let res = collection.add_route(gpx.clone());
                            if res.is_ok() {
                                start_idx_opt = Some(start_idx);
                            }
                            res_opt = res;
                        });
                        res_opt
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        if let Ok(mut collection) = self.route_collection.try_write() {
                            // The route will be appended; record the index where it will be inserted.
                            let start_idx = collection.route_count();
                            let res = collection.add_route(gpx.clone());
                            if res.is_ok() {
                                start_idx_opt = Some(start_idx);
                            }
                            res
                        } else {
                            Err(large_track_lib::DataError::InvalidGeometry(
                                "Could not acquire write lock on route_collection".to_string(),
                            ))
                        }
                    }
                };

                match add_result {
                    Ok(_) => {
                        // Record the starting route index for this file so the UI can map files -> routes.
                        let start_idx = start_idx_opt.unwrap_or(0);
                        self.file_loader.loaded_files.push((path, gpx, start_idx));
                        self.update_stats();
                        self.pending_fit_bounds = true;
                    }
                    Err(e) => {
                        // Format a user-facing error message, push to the error list and set a transient last_error
                        let err_msg = format!("Failed to add route: {}", e);
                        // Push the error record (clone path so we preserve semantics)
                        self.file_loader
                            .errors
                            .push((path.clone(), err_msg.clone()));
                    }
                }
                // No need to increment a processed counter; progress is now based on loaded_files + errors.
            }
            Err(e) => {
                // Preserve the error String for both storage and transient UI feedback.
                self.file_loader.errors.push((path, e));
                self.file_loader
                    .parallel_total_files
                    .fetch_sub(1, Ordering::SeqCst);
                // No need to increment a processed counter; progress is now based on loaded_files + errors.
            }
        }

        // Return true if there are more results to process or still loading
        let more_results = !self
            .file_loader
            .parallel_load_results
            .lock()
            .unwrap()
            .is_empty();
        more_results || self.is_parallel_loading()
    }

    /// Check if parallel loading is in progress
    pub fn is_parallel_loading(&self) -> bool {
        // Use atomic load for the total file count. This is simple, correct,
        // and works uniformly across native and wasm targets.
        let total = self.file_loader.parallel_total_files.load(Ordering::SeqCst);
        total > 0 && self.file_loader.loaded_files.len() < total
    }

    /// Reset parallel loading state (called when all routes are added)
    fn reset_parallel_loading(&mut self) {
        self.file_loader
            .parallel_total_files
            .store(0, Ordering::SeqCst);
    }

    /// Get loading progress (0.0 to 1.0)
    pub fn loading_progress(&self) -> f32 {
        let total = self.file_loader.parallel_total_files.load(Ordering::SeqCst);
        if total == 0 {
            0.0
        } else {
            self.file_loader.loaded_files.len() as f32 / total as f32
        }
    }

    /// Get loading status text
    pub fn loading_status(&self) -> String {
        let total = self.file_loader.parallel_total_files.load(Ordering::SeqCst);
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
    }

    /// Add a file to the pending load queue
    pub fn queue_file(&mut self, dropped_file: DroppedFile) {
        // Use a stable file identifier (prefers real path, falls back to a synthetic web://<name>)
        let file_id = synthetic_path_for(&dropped_file);
        let already_loaded = self
            .file_loader
            .loaded_files
            .iter()
            .any(|(p, _, _)| p == &file_id);

        if !self.file_loader.pending_files.contains(&dropped_file) && !already_loaded {
            self.file_loader.pending_files.push(dropped_file);
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
        #[cfg(not(target_arch = "wasm32"))]
        let old_config = {
            let mut cfg_opt: Option<large_track_lib::Config> = None;
            async_runtime::blocking_read(&self.route_collection, |guard| {
                cfg_opt = Some(guard.config().clone());
            });
            cfg_opt.expect("route_collection read should eventually succeed")
        };
        #[cfg(target_arch = "wasm32")]
        let old_config = match self.route_collection.try_read() {
            Ok(guard) => guard.config().clone(),
            Err(_) => return, // Skip if lock is not available
        };
        let config = Config { bias, ..old_config };
        let mut new_collection = RouteCollection::new(config);

        // Re-add all routes
        for (_, gpx, _) in &self.file_loader.loaded_files {
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
        let total = self.parallel_total_files.load(Ordering::SeqCst);
        let processed = self.loaded_files.len() + self.errors.len();
        !self.pending_files.is_empty() || (total > 0 && processed < total)
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

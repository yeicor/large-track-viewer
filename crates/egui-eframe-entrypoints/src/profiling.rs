/// Profiling integration using tracing-chrome backend
///
/// This module provides profiling support that outputs Chrome trace files
/// which can be viewed with ui.perfetto.dev or chrome://tracing
///
/// The chrome layer is optionally added to the subscriber at runtime using
/// tracing_subscriber::reload. Profiling is controlled by enabling/disabling
/// the chrome layer and managing the FlushGuard lifecycle.
/// The trace file is automatically served over HTTP on port 9001 and
/// Perfetto UI is opened with the trace URL when profiling stops.
use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(feature = "profiling")]
use tracing_chrome::{ChromeLayer, FlushGuard};
#[cfg(feature = "profiling")]
use tracing_subscriber::{Registry, reload};

/// Global state for profiling
#[cfg(feature = "profiling")]
struct ProfilingState {
    /// Handle to reload the chrome layer - when Some, profiling is active
    reload_handle: reload::Handle<Option<ChromeLayer<Registry>>, Registry>,
    /// Guard for the chrome layer - when Some, profiling is active
    guard: Option<FlushGuard>,
    /// Path to the trace file being written
    trace_file: Option<PathBuf>,
    /// Path to the trace file being served (different from trace_file during serving)
    served_trace_file: Option<PathBuf>,
    /// HTTP server thread handle
    http_server: Option<std::thread::JoinHandle<()>>,
    /// Shutdown channel for HTTP server
    shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
}

#[cfg(feature = "profiling")]
static PROFILING_STATE: Mutex<Option<ProfilingState>> = Mutex::new(None);

#[cfg(feature = "profiling")]
fn get_profiling_state() -> &'static Mutex<Option<ProfilingState>> {
    &PROFILING_STATE
}

/// Setup profiling infrastructure at startup
/// This initializes tracing with fmt layer always, and a reloadable chrome layer if profiling feature is enabled.
/// The chrome layer can be enabled/disabled at runtime by reloading the Option<ChromeLayer>.
pub fn setup_logging_and_profiling() {
    use tracing_subscriber::prelude::*;

    // Setup logging
    if std::env::var("RUST_LOG").is_err() {
        // Safety: single-threaded at startup
        unsafe {
            // Nicer default logs
            std::env::set_var("RUST_LOG", "info,wgpu_hal=warn,eframe=warn");
        }
    }

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_filter(tracing_subscriber::EnvFilter::from_default_env());

    #[cfg(feature = "profiling")]
    let registry = {
        // Create a reloadable layer for the optional chrome layer
        // The chrome layer must be added directly to Registry, so we add it first
        let (reload_layer, reload_handle) = reload::Layer::new(None::<ChromeLayer<Registry>>);
        let registry = tracing_subscriber::registry()
            .with(reload_layer)
            .with(fmt_layer);

        // Initialize state with reload handle
        *get_profiling_state().lock().unwrap() = Some(ProfilingState {
            reload_handle,
            guard: None,
            trace_file: None,
            served_trace_file: None,
            http_server: None,
            shutdown_tx: None,
        });

        // An env var can automatically enable profiling at startup if desired
        if std::env::var("ENABLE_PROFILING").is_ok() {
            tracing::info!("ENABLE_PROFILING set - starting profiling session at startup");
            start_profiling();
        }

        tracing::info!("Tracing initialized with reloadable chrome profiling layer");
        registry
    };

    #[cfg(not(feature = "profiling"))]
    let registry = tracing_subscriber::registry().with(fmt_layer);

    registry.init();
}

/// Enable profiling - marks the start of a profiling session
/// This creates a new chrome layer and enables it via reload
#[cfg(feature = "profiling")]
pub fn start_profiling() {
    let state_lock = get_profiling_state();
    let mut state_opt = state_lock.lock().unwrap();
    let state = state_opt.as_mut().expect("Profiling state not initialized");

    if state.trace_file.is_some() {
        tracing::warn!("Profiling already enabled");
        return;
    }

    // Stop HTTP server from previous session and clean up served file
    if let Some(tx) = state.shutdown_tx.take() {
        let _ = tx.send(());
    }
    if let Some(handle) = state.http_server.take() {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = handle.join();
    }

    // Delete previously served trace file
    if let Some(served_file) = state.served_trace_file.take() {
        if let Err(e) = std::fs::remove_file(&served_file) {
            tracing::debug!("Could not delete previous trace file: {}", e);
        } else {
            tracing::info!("Deleted previous trace file: {}", served_file.display());
        }
    }

    // Create chrome layer and guard
    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new().build();

    // Enable the chrome layer via reload
    state
        .reload_handle
        .reload(Some(chrome_layer))
        .expect("Failed to enable chrome layer");

    state.guard = Some(guard);

    tracing::info!("✓ Profiling session started");
    tracing::info!("Chrome layer is capturing all tracing spans");

    // Mark that we have a session active
    state.trace_file = Some(PathBuf::from("_active_"));
}

/// Disable profiling and open Perfetto UI
/// This disables the chrome layer, drops the guard to flush the file, finds the trace file and serves it
#[cfg(feature = "profiling")]
pub fn stop_profiling() {
    let state_lock = get_profiling_state();
    let mut state_opt = state_lock.lock().unwrap();
    let state = state_opt.as_mut().expect("Profiling state not initialized");

    if state.trace_file.is_none() {
        tracing::warn!("Profiling not enabled");
        return;
    }

    tracing::info!("Stopping profiling session...");

    // Disable the chrome layer via reload
    state
        .reload_handle
        .reload(None::<ChromeLayer<Registry>>)
        .expect("Failed to disable chrome layer");

    // Drop the guard to flush the trace file
    state.guard = None;

    state.trace_file = None;

    // Find the most recent trace file
    let trace_file = match find_latest_trace_file() {
        Some(f) => f,
        None => {
            tracing::error!(
                "Could not find trace file! Expected trace-*.json in current directory"
            );
            return;
        }
    };

    tracing::info!("✓ Found trace file: {}", trace_file.display());

    // Check if file exists and has content
    if let Ok(metadata) = std::fs::metadata(&trace_file) {
        if metadata.len() > 10 {
            tracing::info!("✓ Trace file size: {} bytes", metadata.len());
        } else {
            tracing::warn!("⚠ Trace file seems empty ({} bytes)", metadata.len());
        }
    }

    // Store the served file path for later cleanup
    state.served_trace_file = Some(trace_file.clone());

    // Now serve the file and open Perfetto UI
    #[cfg(not(target_arch = "wasm32"))]
    {
        serve_and_open_trace(trace_file, state);
    }
}

/// Find the most recent trace-*.json file in the current directory
#[cfg(feature = "profiling")]
fn find_latest_trace_file() -> Option<PathBuf> {
    use std::fs;

    let current_dir = std::env::current_dir().ok()?;

    let mut trace_files: Vec<_> = fs::read_dir(&current_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|name| name.starts_with("trace-") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            Some((entry.path(), modified))
        })
        .collect();

    trace_files.sort_by_key(|(_, modified)| *modified);
    trace_files.last().map(|(path, _)| path.clone())
}

/// Serve the trace file over HTTP on port 9001 and open Perfetto UI
#[cfg(all(feature = "profiling", not(target_arch = "wasm32")))]
fn serve_and_open_trace(trace_path: PathBuf, state: &mut ProfilingState) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    tracing::info!("Starting HTTP server on port 9001...");

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    // Clone the path for the thread
    let trace_path_clone = trace_path.clone();

    // Start HTTP server in a background thread
    let handle = std::thread::spawn(move || {
        let listener = match TcpListener::bind("127.0.0.1:9001") {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Failed to start HTTP server on port 9001: {}", e);
                tracing::info!("Make sure port 9001 is not already in use");
                return;
            }
        };

        // Set non-blocking so we can check for shutdown
        listener.set_nonblocking(true).ok();

        tracing::info!("✓ HTTP server listening on 127.0.0.1:9001");

        loop {
            // Check for shutdown signal
            if shutdown_rx.try_recv().is_ok() {
                tracing::info!("Shutting down HTTP server");
                break;
            }

            // Accept connections
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    tracing::debug!("Connection from {}", addr);

                    // Read the request to check the method
                    let mut buffer = [0u8; 1024];
                    let bytes_read = stream.read(&mut buffer).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                    // Only listen to our specific GET request, 404 others
                    if request.starts_with("OPTIONS") {
                        // Handle CORS preflight
                        let response = "HTTP/1.1 204 No Content\r\n\
                                       Access-Control-Allow-Origin: *\r\n\
                                       Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                                       Access-Control-Allow-Headers: *\r\n\
                                       Cache-Control: no-cache\r\n\
                                       \r\n";
                        if let Err(e) = stream.write_all(response.as_bytes()) {
                            tracing::warn!("Failed to write OPTIONS response: {}", e);
                        }
                        continue;
                    } else if !request.starts_with("GET / ") {
                        // Not a GET request
                        let response = "HTTP/1.1 404 Not Found\r\n\
                                       Access-Control-Allow-Origin: *\r\n\
                                       \r\n";
                        if let Err(e) = stream.write_all(response.as_bytes()) {
                            tracing::warn!("Failed to write 404 response: {}", e);
                        }
                        continue;
                    }

                    // Handle GET request
                    match std::fs::read(&trace_path_clone) {
                        Ok(contents) => {
                            let response = format!(
                                "HTTP/1.1 200 OK\r\n\
                                 Content-Type: application/json\r\n\
                                 Content-Length: {}\r\n\
                                 Access-Control-Allow-Origin: *\r\n\
                                 Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                                 Access-Control-Allow-Headers: *\r\n\
                                 Cache-Control: no-cache\r\n\
                                 \r\n",
                                contents.len()
                            );

                            if let Err(e) = stream.write_all(response.as_bytes()) {
                                tracing::warn!("Failed to write response headers: {}", e);
                                continue;
                            }

                            if let Err(e) = stream.write_all(&contents) {
                                tracing::warn!("Failed to write response body: {}", e);
                                continue;
                            }

                            if let Err(e) = stream.flush() {
                                tracing::warn!("Failed to flush stream: {}", e);
                            }

                            tracing::debug!("Served trace file ({} bytes)", contents.len());

                            // Delete the trace file after serving
                            if let Err(e) = std::fs::remove_file(&trace_path_clone) {
                                tracing::debug!("Could not delete trace file after serving: {}", e);
                            } else {
                                tracing::info!(
                                    "Deleted trace file after serving: {}",
                                    trace_path_clone.display()
                                );
                            }

                            shutdown_tx.send(()).ok(); // Shutdown server after serving
                        }
                        Err(e) => {
                            tracing::error!("Failed to read trace file: {}", e);
                            let response = "HTTP/1.1 500 Internal Server Error\r\n\
                                           Access-Control-Allow-Origin: *\r\n\
                                           \r\n";
                            let _ = stream.write_all(response.as_bytes());
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection, sleep a bit
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    tracing::warn!("Failed to accept connection: {}", e);
                }
            }
        }
    });

    state.http_server = Some(handle);

    // Give the server a moment to start
    std::thread::sleep(std::time::Duration::from_millis(200));

    let url = "https://ui.perfetto.dev/#!/?url=http://127.0.0.1:9001/";

    tracing::info!("Opening Perfetto UI...");
    tracing::info!("URL: {}", url);

    match open::that(&url) {
        Ok(_) => {
            tracing::info!("✓ Perfetto UI opened in browser");
            tracing::info!("The trace should load automatically!");
        }
        Err(e) => {
            tracing::warn!("Could not auto-open browser: {}", e);
            tracing::info!("Please manually open: {}", url);
        }
    }
}

/// Check if profiling is currently enabled
#[cfg(feature = "profiling")]
pub fn is_profiling_enabled() -> bool {
    let state_lock = get_profiling_state();
    let state_opt = state_lock.lock().unwrap();
    state_opt
        .as_ref()
        .map(|s| s.trace_file.is_some())
        .unwrap_or(false)
}

/// UI widget for profiling controls
pub fn profiling_ui(ui: &mut egui::Ui) {
    #[cfg(feature = "profiling")]
    {
        egui::warn_if_debug_build(ui);

        ui.heading("Profiling (Chrome Tracing)");
        ui.separator();

        let mut enabled = is_profiling_enabled();

        if ui.checkbox(&mut enabled, "Enable Profiling").changed() {
            if enabled {
                start_profiling();
            } else {
                stop_profiling();
            }
        }

        if enabled {
            ui.label("⏺ Recording session active");
            ui.add_space(4.0);
            ui.label("Chrome layer is capturing all tracing spans.");
            ui.label("Disable to view the trace in Perfetto UI.");

            ui.add_space(4.0);
            ui.label("💡 Perform the actions you want to profile now.");
        } else {
            ui.label("Click checkbox to mark profiling session start.");
            ui.add_space(4.0);
            ui.label("⚠️ Note: Chrome layer is enabled on-demand!");
            ui.label("Checkbox controls the layer via reload.");
            ui.add_space(4.0);
            ui.label("When you disable:");
            ui.label("  • Chrome layer is disabled (dropped)");
            ui.label("  • Trace file is flushed");
            ui.label("  • Most recent trace file will be found");
            ui.label("  • HTTP server starts on port 9001");
            ui.label("  • Perfetto UI opens automatically");
            ui.label("  • Trace loads automatically!");
            ui.label("  • Server keeps running until next session");
            ui.label("  • File deleted when starting new profiling");
        }

        ui.add_space(8.0);
        ui.collapsing("About", |ui| {
            ui.label("Uses tracing-chrome to generate Chrome trace files");
            ui.add_space(4.0);
            ui.label("Compatible with:");
            ui.label("  • https://ui.perfetto.dev (recommended)");
            ui.label("  • chrome://tracing");
            ui.add_space(4.0);
            ui.label("Traces include:");
            ui.label("  • Function call hierarchies");
            ui.label("  • Timing information");
            ui.label("  • Custom trace points via tracing macros");
            ui.add_space(4.0);
            ui.label("The 'profiling' crate macros (profiling::scope!, etc.)");
            ui.label("automatically work with tracing infrastructure.");
            ui.add_space(4.0);
            ui.label("⚙️ Technical details:");
            ui.label("  • Chrome layer enabled/disabled at runtime via reload");
            ui.label("  • Checkbox controls layer lifecycle");
            ui.label("  • Trace file auto-generated with timestamp");
            ui.label("  • FlushGuard dropped on disable to flush file");
            ui.label("  • Serves trace on http://127.0.0.1:9001");
            ui.label("  • Handles CORS preflight (OPTIONS) requests");
            ui.label("  • File served multiple times (not deleted after first request)");
            ui.label("  • Port 9001 required by Perfetto's CSP");
        });
    }

    #[cfg(not(feature = "profiling"))]
    {
        ui.label("Profiling feature is disabled in this build.");
        ui.add_space(4.0);
        ui.label("Enable with: --features profiling");
    }
}

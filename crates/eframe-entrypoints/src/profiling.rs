/*!
Profiling and logging integration for eframe-entrypoints.

This module exposes a consistent API regardless of whether the heavy-weight
profiling feature is compiled in. There are two implementations:

- real: compiled only when `feature = "profiling"` are set.
  This provides full tracing-chrome based profiling (reloadable layer,
  FlushGuard lifecycle, trace file serving + Perfetto opening).
- stub: compiled in all other configurations (including release builds).
  This provides no-op profiling functions and a sensible logging-only
  initialization.

Top-level API (always available):
- `setup_logging_and_profiling()`
- `start_profiling()`
- `stop_profiling()`
- `is_profiling_enabled() -> bool`
- `profiling_ui(&mut egui::Ui)`
*/

#[cfg(feature = "profiling")]
mod inner {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use tracing_chrome::{ChromeLayer, FlushGuard};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{Registry, reload};

    // Export a small helper macro to allow other parts of the crate to register
    // the current thread with the profiling backend. This proxies directly to
    // the dependency's `register_thread!` macro using an absolute path so there
    // is no ambiguity between the local `profiling` module and the external crate.
    //
    // Usage:
    //   register_profiling_thread!("WorkerName");
    //
    // Note: the macro expects a string literal.
    #[macro_export]
    macro_rules! register_profiling_thread {
        ($name:literal) => {
            ::profiling::register_thread!($name);
        };
    }

    /// State used by the real profiling implementation.
    struct ProfilingState {
        /// Reload handle for the chrome layer
        reload_handle: reload::Handle<Option<ChromeLayer<Registry>>, Registry>,
        /// Guard which, when dropped, flushes the trace file
        guard: Option<FlushGuard>,
        /// Current active trace file marker (Some while recording)
        trace_file: Option<PathBuf>,
        /// Trace file being served (for cleanup)
        served_trace_file: Option<PathBuf>,
        /// HTTP server thread handle (serves the trace file)
        http_server: Option<std::thread::JoinHandle<()>>,
        /// Shutdown channel to stop the HTTP server
        shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
    }

    static PROFILING_STATE: Mutex<Option<ProfilingState>> = Mutex::new(None);

    fn profiling_state() -> &'static Mutex<Option<ProfilingState>> {
        &PROFILING_STATE
    }

    /// Initialize logging and (optionally) profiling reload layer.
    ///
    /// Behavior:
    /// - If RUST_LOG is not set, set a helpful default.
    /// - Register a reloadable chrome layer so profiling can be toggled at runtime.
    pub fn setup_logging_and_profiling() {
        // Use prelude locally
        use tracing_subscriber::EnvFilter;
        use tracing_subscriber::fmt;

        if std::env::var("RUST_LOG").is_err() {
            // Safety: single-threaded at startup
            unsafe {
                if cfg!(debug_assertions) {
                    std::env::set_var(
                        "RUST_LOG",
                        "debug,eframe::native=warn,hyper_util=info,walkers=info,egui::context=warn,reqwest::connect=info",
                    );
                } else {
                    std::env::set_var("RUST_LOG", "info,eframe::native=warn,egui::context=warn");
                }
            }
            tracing::info!(
                "RUST_LOG set to default: {}",
                std::env::var("RUST_LOG").unwrap()
            );
        }

        let fmt_layer = fmt::layer().with_filter(EnvFilter::from_default_env());

        // Create reloadable chrome layer and attach to registry
        let (reload_layer, reload_handle) = reload::Layer::new(None::<ChromeLayer<Registry>>);
        let registry = tracing_subscriber::registry()
            .with(reload_layer)
            .with(fmt_layer);

        // Save reload handle in shared state
        match profiling_state().lock() {
            Ok(mut guard) => {
                *guard = Some(ProfilingState {
                    reload_handle,
                    guard: None,
                    trace_file: None,
                    served_trace_file: None,
                    http_server: None,
                    shutdown_tx: None,
                });
            }
            Err(poisoned) => {
                tracing::warn!("Profiling state mutex poisoned during init; recovering");
                let mut guard = poisoned.into_inner();
                *guard = Some(ProfilingState {
                    reload_handle,
                    guard: None,
                    trace_file: None,
                    served_trace_file: None,
                    http_server: None,
                    shutdown_tx: None,
                });
            }
        }

        // Optional auto-start if environment variable set
        if std::env::var("ENABLE_PROFILING").is_ok() {
            tracing::info!("ENABLE_PROFILING set - starting profiling session at startup");
            start_profiling();
        }

        tracing::info!("Tracing initialized with reloadable chrome profiling layer (debug)");
        registry.init();
    }

    /// Start a profiling session by enabling the chrome layer and creating a FlushGuard.
    pub fn start_profiling() {
        let state_lock = profiling_state();
        let mut state_opt = match state_lock.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("Profiling state mutex poisoned; recovering");
                poisoned.into_inner()
            }
        };

        let state = match state_opt.as_mut() {
            Some(s) => s,
            None => {
                tracing::error!("Profiling state not initialized");
                return;
            }
        };

        if state.trace_file.is_some() {
            tracing::warn!("Profiling already enabled");
            return;
        }

        // stop any server from previous session
        if let Some(tx) = state.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = state.http_server.take() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = handle.join();
        }

        if let Some(served) = state.served_trace_file.take() {
            if let Err(e) = std::fs::remove_file(&served) {
                tracing::debug!("Could not delete previous served trace file: {}", e);
            } else {
                tracing::info!("Deleted previous served trace file: {}", served.display());
            }
        }

        // Create chrome layer & guard
        let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new().build();

        if let Err(e) = state.reload_handle.reload(Some(chrome_layer)) {
            tracing::error!("Failed to enable chrome layer: {:?}", e);
        }

        state.guard = Some(guard);
        state.trace_file = Some(PathBuf::from("_active_"));

        tracing::info!("✓ Profiling session started (chrome layer enabled)");
    }

    /// Stop profiling: disable chrome layer, drop guard to flush file, then serve the file.
    pub fn stop_profiling() {
        let state_lock = profiling_state();
        let mut state_opt = match state_lock.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("Profiling state mutex poisoned; recovering");
                poisoned.into_inner()
            }
        };

        let state = match state_opt.as_mut() {
            Some(s) => s,
            None => {
                tracing::error!("Profiling state not initialized");
                return;
            }
        };

        if state.trace_file.is_none() {
            tracing::warn!("Profiling not enabled");
            return;
        }

        tracing::info!("Stopping profiling session...");

        if let Err(e) = state.reload_handle.reload(None::<ChromeLayer<Registry>>) {
            tracing::error!("Failed to disable chrome layer: {:?}", e);
        }

        // drop guard to flush
        state.guard = None;
        state.trace_file = None;

        // try to find most recent trace file
        let trace_file = find_latest_trace_file();
        let trace_file = match trace_file {
            Some(f) => f,
            None => {
                tracing::error!(
                    "Could not find trace file! Expected trace-*.json in current directory"
                );
                return;
            }
        };

        tracing::info!("✓ Found trace file: {}", trace_file.display());
        if let Ok(md) = std::fs::metadata(&trace_file) {
            if md.len() > 10 {
                tracing::info!("✓ Trace file size: {} bytes", md.len());
            } else {
                tracing::warn!("⚠ Trace file seems small ({} bytes)", md.len());
            }
        }

        // track served file for cleanup
        state.served_trace_file = Some(trace_file.clone());

        #[cfg(not(target_arch = "wasm32"))]
        {
            serve_and_open_trace(trace_file, state);
        }
    }

    fn find_latest_trace_file() -> Option<PathBuf> {
        use std::fs;

        let current_dir = std::env::current_dir().ok()?;

        let mut trace_files: Vec<_> = fs::read_dir(&current_dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.starts_with("trace-") && s.ends_with(".json"))
                    .unwrap_or(false)
            })
            .filter_map(|entry| {
                let md = entry.metadata().ok()?;
                let mtime = md.modified().ok()?;
                Some((entry.path(), mtime))
            })
            .collect();

        trace_files.sort_by_key(|(_, m)| *m);
        trace_files.last().map(|(p, _)| p.clone())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn serve_and_open_trace(trace_path: PathBuf, state: &mut ProfilingState) {
        use std::io::Read;
        use std::io::Write;
        use std::net::TcpListener;

        tracing::info!("Starting HTTP server on port 9001...");

        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        let trace_path_clone = trace_path.clone();

        let handle = std::thread::spawn(move || {
            // Name the HTTP server thread for profiler backends so traces show a helpful name.
            #[cfg(feature = "profiling")]
            {
                ::profiling::register_thread!("PerfettoHTTP");
            }

            let listener = match TcpListener::bind("127.0.0.1:9001") {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to bind HTTP server on 127.0.0.1:9001: {}", e);
                    return;
                }
            };

            listener.set_nonblocking(true).ok();
            tracing::info!("✓ HTTP server listening on 127.0.0.1:9001");

            loop {
                if shutdown_rx.try_recv().is_ok() {
                    tracing::info!("Shutting down HTTP server");
                    break;
                }

                match listener.accept() {
                    Ok((mut stream, addr)) => {
                        tracing::debug!("Connection from {}", addr);
                        let mut buffer = [0u8; 2048];
                        let bytes_read = stream.read(&mut buffer).unwrap_or(0);
                        let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                        if request.starts_with("OPTIONS") {
                            let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\nCache-Control: no-cache\r\n\r\n";
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        } else if !request.starts_with("GET / ") {
                            let response =
                                "HTTP/1.1 404 Not Found\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        }

                        match std::fs::read(&trace_path_clone) {
                            Ok(contents) => {
                                let response = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\nCache-Control: no-cache\r\n\r\n",
                                    contents.len()
                                );
                                let _ = stream.write_all(response.as_bytes());
                                let _ = stream.write_all(&contents);
                                let _ = stream.flush();

                                tracing::debug!("Served trace file ({} bytes)", contents.len());

                                // Attempt to delete trace file after serving
                                if let Err(e) = std::fs::remove_file(&trace_path_clone) {
                                    tracing::debug!(
                                        "Could not delete trace file after serving: {}",
                                        e
                                    );
                                } else {
                                    tracing::info!(
                                        "Deleted trace file after serving: {}",
                                        trace_path_clone.display()
                                    );
                                }

                                // shut down after serving
                                let _ = shutdown_tx.send(());
                            }
                            Err(e) => {
                                tracing::error!("Failed to read trace file: {}", e);
                                let _ =
                                    stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\n\r\n");
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        tracing::warn!("Failed to accept connection: {}", e);
                    }
                }
            }
        });

        state.http_server = Some(handle);

        // give server a moment to start
        std::thread::sleep(std::time::Duration::from_millis(200));

        let url = "https://ui.perfetto.dev/#!/?url=http://127.0.0.1:9001/";
        tracing::info!("Opening Perfetto UI: {}", url);

        if let Err(e) = open::that(url) {
            tracing::warn!("Could not auto-open browser: {}", e);
            tracing::info!("Please manually open: {}", url);
        } else {
            tracing::info!("✓ Perfetto UI opened in browser");
        }
    }

    pub fn is_profiling_enabled() -> bool {
        let state_lock = profiling_state();
        match state_lock.lock() {
            Ok(guard) => guard
                .as_ref()
                .map(|s| s.trace_file.is_some())
                .unwrap_or(false),
            Err(poisoned) => {
                tracing::warn!(
                    "Profiling state mutex poisoned when checking enabled; treating as disabled"
                );
                poisoned
                    .into_inner()
                    .as_ref()
                    .map(|s| s.trace_file.is_some())
                    .unwrap_or(false)
            }
        }
    }

    pub fn profiling_ui(ui: &mut egui::Ui) {
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
            ui.label("⏺ Recording active");
            ui.label("Capturing tracing spans. Stop to view in Perfetto.");
        } else {
            ui.label("Enable to start profiling.");
        }
    }
}

#[cfg(not(feature = "profiling"))]
mod inner {
    use tracing_subscriber::prelude::*;

    /// Initialize logging with sensible defaults; profiling is a no-op here.
    pub fn setup_logging_and_profiling() {
        use tracing_subscriber::EnvFilter;
        use tracing_subscriber::fmt;

        if std::env::var("RUST_LOG").is_err() {
            // Safety: single-threaded at startup
            unsafe {
                // Release builds default to INFO to avoid excessive logs.
                std::env::set_var("RUST_LOG", "info,eframe=warn");
            }
        }

        let fmt_layer = fmt::layer().with_filter(EnvFilter::from_default_env());
        let registry = tracing_subscriber::registry().with(fmt_layer);
        registry.init();

        tracing::info!("Logging initialized (profiling disabled in this build)");
    }

    pub fn start_profiling() {
        tracing::info!("start_profiling() called but profiling is disabled in this build");
    }

    pub fn stop_profiling() {
        tracing::info!("stop_profiling() called but profiling is disabled in this build");
    }

    pub fn is_profiling_enabled() -> bool {
        false
    }

    pub fn profiling_ui(ui: &mut egui::Ui) {
        ui.label("Profiling feature not enabled in this build.");
    }
}

// Re-export a stable API surface regardless of which `inner` module was compiled.
pub use inner::{
    is_profiling_enabled, profiling_ui, setup_logging_and_profiling, start_profiling,
    stop_profiling,
};

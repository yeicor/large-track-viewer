//! Cross-platform entry points system for egui/eframe applications
//!
//! This crate provides reusable entry points for native (desktop/mobile) and web platforms,
//! along with utilities for CLI parsing, profiling, and metadata display.
//!
//! # Usage
//!
//! In your application's `lib.rs`, use the `eframe_app!` macro to define all entry points:
//!
//! ```ignore
//! eframe_entrypoints::eframe_app!(
//!     "My App Name",
//!     |cc| Box::new(MyApp::new(cc))
//! );
//! ```
//!
//! This generates:
//! - Web: `create_egui_app` function for WASM builds
//! - Android: `android_main` entry point
//! - Native: `run_native()` function to call from `main.rs`
//!
//! In your `main.rs`:
//!
//! ```ignore
//! fn main() {
//!     my_app::run_native();
//! }
//! ```

pub mod async_runtime;
pub mod cli;
pub mod profiling;
pub mod run;

// Re-export commonly used types
pub use cli::parse_args;
pub use profiling::profiling_ui;

/// Convenience macro to create profiling scopes from other crates/modules.
/// Usage:
///   app_profiling_scope!("some_scope_name");
///
/// This proxies to the profiling crate using an absolute path to avoid
/// ambiguity with the local `profiling` module re-export.
#[macro_export]
macro_rules! app_profiling_scope {
    ($name:literal) => {
        #[cfg(feature = "profiling")]
        ::profiling::scope!($name);
    };
}

// Profiling macros will be referenced via absolute crate path (::profiling::...)
// The explicit `extern crate` alias was removed to avoid name conflicts with the

mod metadata;
pub use metadata::{log_version_info, short_version_info};

#[cfg(target_arch = "wasm32")]
pub mod web;

// Re-export eframe types commonly needed for app creation
pub use eframe;
pub use eframe::CreationContext;

/// Unified macro to define all platform entry points for an eframe application.
///
/// This macro generates the necessary entry points for web (WASM), Android, and native
/// (desktop) platforms with a single, consistent API.
///
/// # Arguments
///
/// * `$app_name` - A string literal with the application name (used for window title, logging, etc.)
/// * `$app_creator` - A closure that takes `&CreationContext` and returns `Box<dyn eframe::App>`
///
/// # Example
///
/// ```ignore
/// use eframe_entrypoints::eframe_app;
///
/// pub struct MyApp;
///
/// impl MyApp {
///     pub fn new(_cc: &eframe_entrypoints::CreationContext<'_>) -> Self {
///         Self
///     }
/// }
///
/// impl eframe::App for MyApp {
///     fn update(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {}
/// }
///
/// eframe_app!("My Application", |cc| Box::new(MyApp::new(cc)));
/// ```
///
/// # Generated Code
///
/// For **Web (WASM)** targets, generates:
/// ```ignore
/// #[no_mangle]
/// pub fn create_egui_app(cc: &eframe::CreationContext<'_>) -> Box<dyn eframe::App> { ... }
/// ```
///
/// For **Android** targets, generates:
/// ```ignore
/// #[no_mangle]
/// pub fn android_main(app: winit::platform::android::activity::AndroidApp) { ... }
/// ```
///
/// For **all targets**, generates:
/// ```ignore
/// pub fn run_native() { ... }  // Call this from main.rs
/// ```
#[macro_export]
macro_rules! eframe_app_lib {
    ($app_name:expr, $app_creator:expr) => {
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)] // SAFETY: there is no other global function of this name
        pub fn android_main(app: ::winit::platform::android::activity::AndroidApp) {
            $crate::android_main_impl($app_name, app, $app_creator);
        }
    };
}

#[macro_export]
macro_rules! eframe_app_main {
    ($app_name:expr, $app_creator:expr) => {
        fn main() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let rt = ::tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create Tokio runtime");

                rt.block_on(async {
                    $crate::native_main_impl($app_name, $app_creator).await;
                });
            };
        }

        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn main_web() {
            $crate::web::set_app_creator($app_creator);
        }
    };
}

/// Internal implementation for Android entry point.
/// Use the `eframe_app!` macro instead of calling this directly.
///
/// Notes:
/// - When compiled in debug with the `profiling` feature, this function is
///   instrumented and will register the main Android entry thread with the
///   profiling backend so that traces show a meaningful thread name.
/// - The `cfg_attr` ensures the profiling attribute is only applied in
///   debug builds with the `profiling` feature enabled.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub fn android_main_impl(
    app_name: &str,
    app: winit::platform::android::activity::AndroidApp,
    app_creator: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + Send + 'static,
) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default());
    log::info!("Starting {} on Android", app_name);

    // Name the thread and create a short-lived scope for the Android main entry so
    // profiler traces contain a clear entry point for application startup.
    #[cfg(feature = "profiling")]
    {
        ::profiling::register_thread!("AndroidMain");
        ::profiling::scope!("app::android_main_impl");
    }

    // Register the main thread name for logging/profiling in debug builds.
    // When the `profiling` feature is enabled we register this thread so that
    // profiler UIs can show a human-friendly thread name in traces.
    #[cfg(feature = "profiling")]
    {
        // The profiling crate exposes a `register_thread` macro to name the current
        // thread for profiler backends. Use a stable name for the Android main
        // entry so traces include it.
        //
        // If your profiling crate exposes a different API (function vs macro),
        // adjust this call accordingly.
        ::profiling::register_thread!("AndroidMain");
    }

    unsafe {
        // Safe: single-threaded at startup
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let app_name_owned = app_name.to_string();
    rt.block_on(async {
        log_version_info();

        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_title(&app_name_owned),
            event_loop_builder: Some(Box::new(move |builder| {
                builder.with_android_app(app);
            })),
            ..Default::default()
        };

        let _ = eframe::run_native(
            &app_name_owned,
            native_options,
            Box::new(move |cc| Ok(app_creator(cc))),
        );
    });
}

/// Internal implementation for native (desktop) entry point.
/// Use the `eframe_app!` macro instead of calling this directly.
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub async fn native_main_impl(
    app_name: &str,
    app_creator: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
) {
    // Create a profiling scope marking native main startup so startup time and
    // initialization work are visible in traces. This is a no-op when profiling
    // is disabled.
    #[cfg(feature = "profiling")]
    {
        ::profiling::scope!("app::native_main_impl");
    }

    // Initialize tracing subscriber with profiling support if enabled
    // This MUST be done before any logging, so both fmt and chrome layers
    // are registered together in the same subscriber
    profiling::setup_logging_and_profiling();

    // Register the main thread name for logging/profiling in debug builds.
    // When the `profiling` feature is enabled we register this thread so that
    // profiler UIs can show a human-friendly thread name in traces.
    #[cfg(feature = "profiling")]
    {
        // Name the main native thread. If your profiling crate uses a function
        // instead of a macro, update this line to call the correct API.
        ::profiling::register_thread!("Main");
    }

    log_version_info();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title(app_name)
            .with_drag_and_drop(true),
        ..Default::default()
    };

    let _ = eframe::run_native(
        app_name,
        native_options,
        Box::new(move |cc| Ok(app_creator(cc))),
    );
}

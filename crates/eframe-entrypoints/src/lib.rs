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

/// Reusable web file picker utilities (implemented in `src/web_file_picker.rs`).
pub mod file_picker;

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
        // Android entry point - matches sdf-viewer's approach:
        // - Uses #[no_mangle] (not #[unsafe(no_mangle)]) for compatibility
        // - Non-pub function as expected by android-activity crate
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)] // SAFETY: there is no other global function of this name
        pub fn android_main(app: ::winit::platform::android::activity::AndroidApp) {
            $crate::run::android_main($app_name, app, $app_creator);
        }
    };
}

#[macro_export]
macro_rules! eframe_app_main {
    ($app_name:expr, $app_creator:expr) => {
        fn main() {
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
            {
                $crate::run::desktop_main($app_name, $app_creator);
            };
        }

        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn web_main() {
            $crate::web::set_app_creator($app_creator);
        }
    };
}

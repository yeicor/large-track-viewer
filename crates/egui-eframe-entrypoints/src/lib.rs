//! Cross-platform entry points system for egui/eframe applications
//!
//! This crate provides reusable entry points for native (desktop/mobile) and web platforms,
//! along with utilities for CLI parsing, profiling, and metadata display.

pub mod cli;
pub mod profiling;
pub mod run;

// Re-export commonly used types
pub use cli::parse_args;
pub use profiling::profiling_ui;

mod metadata;
pub use metadata::{log_version_info, short_version_info};

#[cfg(target_arch = "wasm32")]
pub mod web;

/// Entry point for Android
#[cfg(target_os = "android")]
pub fn android_main(
    app: winit::platform::android::activity::AndroidApp,
    app_creator: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + Send + 'static,
) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default());
    log::info!("Starting eframe application on Android");

    unsafe {
        // Safe: single-threaded at startup
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        log_version_info();

        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_title("eframe app"),
            event_loop_builder: Some(Box::new(move |builder| {
                builder.with_android_app(app);
            })),
            ..Default::default()
        };

        let _ = eframe::run_native(
            "eframe app",
            native_options,
            Box::new(move |cc| Ok(app_creator(cc))),
        );
    });
}

/// Entry point for desktop/native platforms
#[cfg(not(target_arch = "wasm32"))]
pub async fn native_main(
    app_name: &str,
    app_creator: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
) {
    // Initialize tracing subscriber with profiling support if enabled
    // This MUST be done before any logging, so both fmt and chrome layers
    // are registered together in the same subscriber
    profiling::setup_logging_and_profiling();

    log_version_info();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title(app_name),
        ..Default::default()
    };

    let _ = eframe::run_native(
        app_name,
        native_options,
        Box::new(move |cc| Ok(app_creator(cc))),
    );
}

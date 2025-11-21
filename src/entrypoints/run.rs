use super::metadata::log_version_info;

/// Setup and create the app
#[allow(dead_code)]
pub async fn setup_app()
-> Option<Box<dyn FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>>> {
    log_version_info();
    Some(Box::new(|cc| {
        Box::new(crate::app::LargeTrackViewerApp::new(cc))
    }))
}

/// Native entry point
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
pub async fn native_main() {
    // Setup logging
    if std::env::var("RUST_LOG").is_err() {
        // Safety: single-threaded at startup
        unsafe {
            // Nicer default logs
            std::env::set_var("RUST_LOG", "info,wgpu_hal=warn,eframe=warn");
        }
    }
    tracing_subscriber::fmt::init(); // Transform logs into tracing events

    if let Some(app_creator) = setup_app().await {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 720.0])
                .with_title("Large Track Viewer"),
            ..Default::default()
        };

        let _ = eframe::run_native(
            "Large Track Viewer",
            native_options,
            Box::new(move |cc| Ok(app_creator(cc))),
        );
    }
}

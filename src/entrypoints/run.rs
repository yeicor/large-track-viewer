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
    tracing_subscriber::fmt::init();

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

//! Large Track Viewer - Application Library
//!
//! This is the main application crate that integrates the data structures
//! and entry points to create the complete GPS track viewer application.

mod app;

pub use app::LargeTrackViewerApp;

// Entry point for Android
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    egui_eframe_entrypoints::android_main(app, |cc| Box::new(LargeTrackViewerApp::new(cc)));
}

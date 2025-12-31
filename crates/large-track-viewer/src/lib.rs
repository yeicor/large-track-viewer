//! Large Track Viewer - Application Library
//!
//! This is the main application crate that integrates the data structures
//! and entry points to create the complete GPS track viewer application.

mod app;

pub use app::LargeTrackViewerApp;

// Define all platform entry points using the unified macro
eframe_entrypoints::eframe_app!("Large Track Viewer", |cc| Box::new(
    LargeTrackViewerApp::new(cc)
));

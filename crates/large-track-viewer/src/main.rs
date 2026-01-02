#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod app;

pub use app::LargeTrackViewerApp;

eframe_entrypoints::eframe_app_main!("Large Track Viewer", |cc| Box::new(
    LargeTrackViewerApp::new(cc)
));

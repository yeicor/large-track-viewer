#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// The binary uses the library, not duplicate modules
use large_track_viewer::LargeTrackViewerApp;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            egui_eframe_entrypoints::native_main("Large Track Viewer", |cc| {
                Box::new(LargeTrackViewerApp::new(cc))
            })
            .await;
        });
    }
}

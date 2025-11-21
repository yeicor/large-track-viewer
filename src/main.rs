#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// The binary uses the library, not duplicate modules
use large_track_viewer::entrypoints;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    entrypoints::main::main();
}

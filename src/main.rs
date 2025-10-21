#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod app;
mod entrypoints;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    entrypoints::main::main();
}

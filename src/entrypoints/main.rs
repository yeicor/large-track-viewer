#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// === Entry point for desktop ===
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main(flavor = "multi_thread")]
pub async fn main() {
    super::run::native_main().await;
}

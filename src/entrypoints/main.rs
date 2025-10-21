// === Entry point for desktop ===
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main(flavor = "multi_thread")]
pub async fn main() {
    super::run::native_main().await;
}

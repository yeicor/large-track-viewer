//! Cross-platform async runtime abstraction
//!
//! This module provides a unified API for spawning async tasks that works
//! on both native (using tokio) and web (using tokio-with-wasm) platforms.
//!
//! On web, tokio-with-wasm runs async tasks on the JavaScript event loop
//! and can spawn blocking tasks to web workers.

// Re-export sync primitives - these work on both platforms since they're
// just async primitives that work with any executor
pub use tokio::sync::{RwLock, Semaphore};

/// Spawn an async task.
///
/// On native: Uses tokio's multi-threaded runtime
/// On web: Uses tokio-with-wasm which runs on the JS event loop
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(future)
}

/// Spawn an async task.
///
/// On native: Uses tokio's multi-threaded runtime
/// On web: Uses tokio-with-wasm which runs on the JS event loop
#[cfg(target_arch = "wasm32")]
pub fn spawn<F>(future: F) -> tokio_with_wasm::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio_with_wasm::spawn(future)
}

/// Yield execution to allow other tasks to run.
///
/// On native: Uses tokio's yield_now
/// On web: Uses tokio-with-wasm's yield_now which yields to the JS event loop
#[cfg(not(target_arch = "wasm32"))]
pub async fn yield_now() {
    tokio::task::yield_now().await
}

/// Yield execution to allow other tasks to run.
///
/// On native: Uses tokio's yield_now
/// On web: This is a no-op because the JS event loop handles task scheduling
/// automatically. The tokio_with_wasm::task::yield_now() uses JsFuture which
/// is not Send, making it incompatible with spawn() that requires Send futures.
#[cfg(target_arch = "wasm32")]
pub async fn yield_now() {
    // On web, the JS event loop handles scheduling automatically.
    // We don't need an explicit yield since async tasks are cooperative anyway.
    // Note: tokio_with_wasm::task::yield_now() exists but uses JsFuture which
    // is not Send, so we can't use it in spawned tasks.
}

/// Check if we're running inside a tokio runtime context (native only).
/// On web, this always returns true since tasks run on the JS event loop.
#[cfg(not(target_arch = "wasm32"))]
pub fn in_runtime_context() -> bool {
    tokio::runtime::Handle::try_current().is_ok()
}

/// Check if we're running inside a tokio runtime context (native only).
/// On web, this always returns true since tasks run on the JS event loop.
#[cfg(target_arch = "wasm32")]
pub fn in_runtime_context() -> bool {
    // On web, we're always "in context" since tokio-with-wasm uses the JS event loop
    true
}

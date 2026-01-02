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
    // Wrap the provided future in a profiling scope so spawned tasks are easier
    // to identify in profiling traces. When profiling is disabled this is a no-op.
    #[cfg(feature = "profiling")]
    {
        tokio::spawn(async move {
            // Attach a tag describing the spawned future type so profiler traces
            // can be filtered by task kind without emitting separate events.
            profiling::scope!(
                "async_runtime::spawn",
                format!("task_type={}", std::any::type_name::<F>()).as_str()
            );
            future.await
        })
    }
    #[cfg(not(feature = "profiling"))]
    {
        tokio::spawn(future)
    }
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
    // On wasm the same wrapper pattern applies: create a profiling scope inside
    // the spawned task so it appears in traces when profiling is enabled.
    #[cfg(feature = "profiling")]
    {
        tokio_with_wasm::spawn(async move {
            profiling::scope!("async_runtime::spawn");
            future.await
        })
    }
    #[cfg(not(feature = "profiling"))]
    {
        tokio_with_wasm::spawn(future)
    }
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
/// On web, this always returns true since tokio-with-wasm uses the JS event loop.
#[cfg(target_arch = "wasm32")]
pub fn in_runtime_context() -> bool {
    // On web, we're always "in context" since tokio-with-wasm uses the JS event loop
    true
}

// ---------------------------------------------------------------------------
// Lock helpers
//
// Provide safe cooperative helpers for acquiring async RwLock access in a
// way that avoids silent failure when callers used `try_read`/`try_write`.
//
// - `with_read` / `with_write` are async helpers that await the lock and then
//   invoke a provided closure while holding the guard. These avoid returning
//   the guard across an await point (which is not possible for `async fn`).
// - `blocking_read` / `blocking_write` are synchronous helpers available only
//   on native targets; they spin using `try_read`/`try_write` and yield the
//   thread between attempts. Spinning on wasm would hang the event loop, so
//   these are intentionally not provided on wasm.
// ---------------------------------------------------------------------------

/// Acquire a read lock and run a closure while holding it.
/// This is the recommended approach on both native and web (await the future).
/// The closure runs synchronously while the guard is held.
pub async fn with_read<T, R, F>(lock: &RwLock<T>, f: F) -> R
where
    F: FnOnce(&T) -> R + Send,
    R: Send,
{
    let guard = lock.read().await;
    f(&*guard)
}

/// Acquire a write lock and run a closure while holding it.
/// This is the recommended approach on both native and web (await the future).
/// The closure runs synchronously while the guard is held.
pub async fn with_write<T, R, F>(lock: &RwLock<T>, f: F) -> R
where
    F: FnOnce(&mut T) -> R + Send,
    R: Send,
{
    let mut guard = lock.write().await;
    f(&mut *guard)
}

// Synchronous blocking helpers (native-only).
// These repeatedly attempt `try_read`/`try_write` and yield the current thread
// between attempts. This ensures the operation will eventually complete on
// native platforms while avoiding silent skipping of work when a lock is busy.
#[cfg(not(target_arch = "wasm32"))]
pub fn blocking_read<T, R, F>(lock: &RwLock<T>, f: F) -> R
where
    F: FnOnce(&T) -> R,
{
    // Top-level profiling scope for the blocking read helper.
    // Attach a static tag so lock-wait hotspots can be filtered by helper name.
    #[cfg(feature = "profiling")]
    profiling::scope!("async_runtime::blocking_read", "helper=blocking_read");

    loop {
        if let Ok(guard) = lock.try_read() {
            return f(&*guard);
        }
        // Let the scheduler run other threads/tasks before retrying
        std::thread::yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn blocking_write<T, R, F>(lock: &RwLock<T>, f: F) -> R
where
    F: FnOnce(&mut T) -> R,
{
    // Profiling scope for the blocking write helper so lock contention is visible.
    #[cfg(feature = "profiling")]
    profiling::scope!("async_runtime::blocking_write");

    loop {
        if let Ok(mut guard) = lock.try_write() {
            return f(&mut *guard);
        }
        // Let the scheduler run other threads/tasks before retrying
        std::thread::yield_now();
    }
}

// Note for wasm:
// - We do NOT provide spinning/blocking helpers on wasm because blocking the
//   browser main thread would freeze the UI and starve the event loop.
// - On web, callers should use `with_read` / `with_write` (await) to safely
//   acquire locks cooperatively.

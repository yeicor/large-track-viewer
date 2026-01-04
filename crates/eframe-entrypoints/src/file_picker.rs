//! Cross-platform reusable file picker utilities.
//!
//! This module exposes a simple cross-platform API for showing a file picker
//! and retrieving selected files as (name, bytes).
//!
//! - On native (desktop) and wasm (web) targets: uses `rfd` async API to show
//!   the file picker, reads selected files into memory and enqueues them for retrieval.
//! - On Android: uses JNI to call a Java bridge class that shows the file picker.
//!
//! The shared queue is implemented with `once_cell::sync::Lazy` + `Mutex` so
//! callers can call `open_file_picker(...)` followed by `drain_file_queue()`
//! to obtain newly selected files in a uniform way.

use once_cell::sync::Lazy;
use std::sync::Mutex;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

/// Shared queue of picked files. Each entry is (name, bytes).
type QueueEntry = (String, Vec<u8>);
type Queue = Vec<QueueEntry>;
static QUEUE: Lazy<Mutex<Queue>> = Lazy::new(|| Mutex::new(Vec::new()));

#[cfg(target_os = "android")]
pub static ANDROID_APP: Lazy<Mutex<Option<AndroidApp>>> = Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "android")]
pub static PERMISSION_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(not(target_os = "android"))]
mod rfd {
    use super::QUEUE;

    /// Async implementation that uses rfd's AsyncFileDialog.
    /// Works on both native (desktop) and wasm (web) targets.
    async fn open_file_picker_async(accept: Option<&str>, multiple: bool) -> Result<(), String> {
        let mut dialog = rfd::AsyncFileDialog::new();

        if let Some(acc) = accept {
            // If accept starts with '.', treat as extension filter
            if let Some(ext) = acc.strip_prefix('.') {
                dialog = dialog.add_filter(format!("{} files", ext), &[ext]);
            }
        }

        let handles = if multiple {
            dialog.pick_files().await
        } else {
            dialog.pick_file().await.map(|h| vec![h])
        };

        if let Some(handles) = handles {
            for handle in handles {
                let name = handle.file_name();
                let bytes = handle.read().await;
                if let Ok(mut guard) = QUEUE.lock() {
                    guard.push((name, bytes));
                }
            }
        }

        Ok(())
    }

    /// Open the file picker using rfd's async API.
    /// On wasm, spawns via wasm_bindgen_futures.
    /// On native, spawns a thread and blocks on the future.
    pub fn open_file_picker(accept: Option<&str>, multiple: bool) -> Result<(), String> {
        // Clone accept string for the async block
        let accept_owned = accept.map(|s| s.to_string());

        // Reuse the crate's shared async runtime abstraction to run the rfd async picker.
        // The picker uses JS futures on web which are !Send, so we must use
        // `wasm_bindgen_futures::spawn_local` on wasm. On native, use the crate
        // runtime wrapper which expects a `Send` future.
        let fut = async move {
            let _ = open_file_picker_async(accept_owned.as_deref(), multiple).await;
        };

        // Spawn using the crate's async runtime on all targets.
        let _ = crate::async_runtime::spawn(fut);
        Ok(())
    }
}

pub(crate) mod rust {
    use egui_file_dialog::FileDialog;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    #[cfg(target_os = "android")]
    use crate::file_picker::{ANDROID_APP, PERMISSION_REQUESTED};

    static DIALOG: Lazy<Mutex<Option<FileDialog>>> = Lazy::new(|| Mutex::new(None));

    pub fn open_file_picker(accept: Option<&str>, multiple: bool) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            if !PERMISSION_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(app) = ANDROID_APP.lock().unwrap().as_ref() {
                    crate::file_picker::rust::request_storage_permission(app.clone())
                        .expect("failed to request storage permission");
                    PERMISSION_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }

        #[cfg(target_os = "android")]
        let default_dir = std::path::PathBuf::from("/sdcard");
        #[cfg(not(target_os = "android"))]
        let default_dir = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut dialog = FileDialog::new().initial_directory(default_dir);

        if let Some(acc) = accept {
            // egui-file-dialog supports filters; for simplicity, treat as name filter if starts with '.'
            if let Some(ext) = acc.strip_prefix('.') {
                // Make an owned string so the closure can be 'static when wrapped in Arc.
                let ext_owned = ext.to_string();
                // Create a label owned by this stack frame for the call (it's not stored beyond the call).
                let label = format!("{} files", ext_owned);
                dialog = dialog.default_file_filter(&label).add_file_filter(
                    label.as_str(),
                    std::sync::Arc::new(move |p: &std::path::Path| {
                        p.extension().and_then(|s| s.to_str()).unwrap_or_default() == ext_owned
                    }),
                );
            }
        }

        if multiple {
            dialog.pick_multiple();
        } else {
            dialog.pick_file();
        }

        *DIALOG.lock().unwrap() = Some(dialog);
        Ok(())
    }

    #[cfg(target_os = "android")]
    pub(crate) fn request_storage_permission(
        app: winit::platform::android::activity::AndroidApp,
    ) -> Result<(), String> {
        use jni::{
            JavaVM,
            objects::{JObject, JValue},
            sys,
        };

        tracing::info!("Starting all-files access request");

        // Get the JavaVM
        let vm_ptr = app.vm_as_ptr() as *mut *const sys::JNIInvokeInterface_;
        let vm = unsafe { JavaVM::from_raw(vm_ptr) }
            .map_err(|e| format!("Failed to get JavaVM: {:?}", e))?;
        tracing::debug!("Obtained JavaVM");

        // Attach to current thread
        let mut env = vm
            .attach_current_thread()
            .map_err(|e| format!("Failed to attach thread: {:?}", e))?;
        tracing::debug!("Attached to current thread");

        // Get the activity
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as sys::jobject) };
        tracing::debug!("Obtained activity object");

        // Check if already granted
        let environment_class = env
            .find_class("android/os/Environment")
            .map_err(|e| format!("Failed to find Environment class: {:?}", e))?;
        let is_granted = env
            .call_static_method(&environment_class, "isExternalStorageManager", "()Z", &[])
            .map_err(|e| format!("Failed to call isExternalStorageManager: {:?}", e))?;
        if is_granted
            .z()
            .map_err(|e| format!("Failed to get boolean: {:?}", e))?
        {
            tracing::info!("All-files access already granted");
            return Ok(());
        }

        // Find the Intent class
        let intent_class = env
            .find_class("android/content/Intent")
            .map_err(|e| format!("Failed to find Intent class: {:?}", e))?;
        tracing::debug!("Found Intent class");

        // Create the action string
        let action_string = env
            .new_string("android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION")
            .map_err(|e| format!("Failed to create action string: {:?}", e))?;

        // Create Intent
        let intent = env
            .new_object(
                &intent_class,
                "(Ljava/lang/String;)V",
                &[JValue::Object(&action_string)],
            )
            .map_err(|e| format!("Failed to create Intent: {:?}", e))?;
        tracing::debug!("Created Intent");

        // Call startActivity
        env.call_method(
            &activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )
        .map_err(|e| format!("Failed to call startActivity: {:?}", e))?;
        tracing::info!("Called startActivity successfully");

        Ok(())
    }

    /// Helper to hook into GUI rendering code. Call this in your egui UI to show the file dialog.
    /// It will handle selection and enqueue files automatically.
    pub fn render_file_dialog(ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        let mut drop_dialog = false;
        if let Some(dialog) = DIALOG.lock().unwrap().as_mut() {
            dialog.update(ctx);
            // No support for file operations on web
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(paths) = dialog
                .take_picked_multiple()
                .or_else(move || dialog.take_picked().map(|p| vec![p]))
            {
                // Offload file reads to the async runtime so we don't block the UI thread.
                for path in paths {
                    // Capture the file name before moving `path` into the async task.
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let path_for_task = path.clone();

                    // Spawn an async task via the crate runtime and use tokio's async file API
                    // to read the file without blocking threads.
                    let _ = crate::async_runtime::spawn(async move {
                        if let Ok(bytes) = tokio::fs::read(path_for_task).await {
                            if let Ok(mut guard) = super::QUEUE.lock() {
                                guard.push((name, bytes));
                            }
                        }
                        // ignore errors; nothing to do on failure
                    });
                }
                drop_dialog = true;
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if drop_dialog {
            *DIALOG.lock().unwrap() = None;
        }
    }
}

#[cfg(not(target_os = "android"))]
pub use rfd::open_file_picker as open_native_file_picker;
pub use rust::open_file_picker as open_rust_file_picker;
pub use rust::render_file_dialog as render_rust_file_dialog;

/// Drain the shared Rust-side queue and return all picked files.
#[allow(dead_code)]
pub fn drain_file_queue() -> Result<Vec<(String, Vec<u8>)>, String> {
    if let Ok(mut guard) = QUEUE.lock() {
        let out = guard.drain(..).collect();
        Ok(out)
    } else {
        Err("failed to lock queue".to_string())
    }
}

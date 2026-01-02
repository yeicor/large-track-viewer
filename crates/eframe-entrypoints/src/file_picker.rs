//! Cross-platform reusable file picker utilities.
//!
//! This module exposes a simple cross-platform API for showing a file picker
//! and retrieving selected files as (name, bytes).
//!
//! - On native (desktop) targets: uses `rfd` to show the native file picker,
//!   reads selected files into memory and enqueues them for retrieval.
//! - On wasm (web) targets: uses an invisible `<input type="file">` and
//!   `FileReader` to read files, pushing bytes into a shared Rust-side queue.
//!
//! The shared queue is implemented with `once_cell::sync::Lazy` + `Mutex` so
//! callers can call `open_file_picker(...)` followed by `drain_file_queue()`
//! to obtain newly selected files in a uniform way.

use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Shared queue of picked files. Each entry is (name, bytes).
type QueueEntry = (String, Vec<u8>);
type Queue = Vec<QueueEntry>;
static QUEUE: Lazy<Mutex<Queue>> = Lazy::new(|| Mutex::new(Vec::new()));

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::QUEUE;
    use js_sys::Uint8Array;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{FileReader, HtmlInputElement};

    /// Open the browser file picker and read selected files into the shared queue.
    /// `accept` and `multiple` behave like the native input attributes.
    pub fn open_file_picker(accept: Option<&str>, multiple: bool) -> Result<(), String> {
        let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
        let document = window.document().ok_or_else(|| "no document".to_string())?;

        let input_elem = document
            .create_element("input")
            .map_err(|e| format!("create_element failed: {:?}", e))?;
        let input: HtmlInputElement = input_elem
            .dyn_into::<HtmlInputElement>()
            .map_err(|e| format!("dyn_into HtmlInputElement failed: {:?}", e))?;

        input.set_type("file");
        input.set_multiple(multiple);
        if let Some(acc) = accept {
            input.set_accept(acc);
        }
        input.style().set_property("display", "none").ok();

        let input_for_closure = input.clone();
        let onchange = Closure::wrap(Box::new(move |_evt: web_sys::Event| {
            if let Some(files) = input_for_closure.files() {
                for i in 0..files.length() {
                    if let Some(file) = files.get(i) {
                        let file_name = file.name();
                        let reader = FileReader::new().expect("failed to create FileReader");
                        let reader_clone = reader.clone();
                        // Use Rc so the filename can be cheaply cloned inside the onload closure
                        let name_clone = Rc::new(file_name);

                        let onload = Closure::wrap(Box::new(move |_e: web_sys::ProgressEvent| {
                            if let Ok(result) = reader_clone.result() {
                                // Convert ArrayBuffer -> Vec<u8>
                                let uint8 = Uint8Array::new(&result);
                                let mut vec = vec![0u8; uint8.length() as usize];
                                uint8.copy_to(&mut vec);
                                if let Ok(mut guard) = QUEUE.lock() {
                                    // clone the String out of the Rc without moving the Rc itself
                                    guard.push((name_clone.as_ref().clone(), vec));
                                }
                            }
                        }) as Box<dyn FnMut(_)>);

                        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                        let _ = reader.read_as_array_buffer(&file);
                        onload.forget();
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);

        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget();

        if let Some(body) = document.body() {
            let _ = body.append_child(&input);
            let _ = input.click();

            // Schedule removal
            let input_for_removal = input.clone();
            let remover = Closure::wrap(Box::new(move || {
                if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                    if let Some(body) = doc.body() {
                        let _ = body.remove_child(&input_for_removal);
                    }
                }
            }) as Box<dyn Fn()>);
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                remover.as_ref().unchecked_ref(),
                500,
            );
            remover.forget();
        }

        Ok(())
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod imp {
    use super::QUEUE;
    use std::path::PathBuf;

    /// Open native file picker using rfd, read selected files to bytes and enqueue.
    pub fn open_file_picker(accept: Option<&str>, multiple: bool) -> Result<(), String> {
        // Map simple accept like ".gpx" -> extension filter for rfd
        let mut dialog = rfd::FileDialog::new();
        if let Some(acc) = accept {
            // naive: if acc starts with '.', treat as extension
            if let Some(ext) = acc.strip_prefix('.') {
                dialog = dialog.add_filter(format!("{} files", ext), &[ext]);
            } else {
                // otherwise ignore (rfd supports glob patterns in filters but we'll keep simple)
            }
        }
        let paths: Option<Vec<PathBuf>> = if multiple {
            dialog.pick_files()
        } else {
            dialog.pick_file().map(|p| vec![p])
        };

        if let Some(paths) = paths {
            for path in paths {
                // Read file to bytes
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let name = path
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "file".to_string());
                        if let Ok(mut guard) = QUEUE.lock() {
                            guard.push((name, bytes));
                        }
                    }
                    Err(e) => {
                        return Err(format!("failed to read file {:?}: {}", path, e));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "android")]
mod imp {
    pub fn open_file_picker(_accept: Option<&str>, _multiple: bool) -> Result<(), String> {
        Err("Android unsupported".to_string())
    }
}

pub use imp::open_file_picker;

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

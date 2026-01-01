// large-track-viewer/crates/large-track-viewer/src/app/storage/mod.rs
//! Storage abstraction used by the app.
//!
//! This module provides a single trait `StorageBackend` and two concrete
//! implementations:
//!
//! - `WebLocalStorage` (compiled for `wasm32`) — uses `window.localStorage`
//!   to store string key/value pairs (suitable for the browser).
//! - `FileStorage` (compiled for native targets) — stores a single JSON file
//!   containing a map of string keys to string values. The file is located in
//!   a sensible per-user configuration directory (where possible) and is
//!   read/written synchronously.
//!
//! The abstraction exposes string-level APIs and convenient `save_json`/`load_json`
//! helpers that use `serde` for serializing/deserializing structured data.
//!
//! The app code should use the trait rather than directly talking to e.g.
//! `eframe` storage so we can persist to either backend depending on platform.

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    #[cfg(not(target_arch = "wasm32"))]
    Io(String),

    #[error("JSON error: {0}")]
    Json(String),

    #[error("Platform storage error: {0}")]
    Platform(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// Simple generic storage backend trait.
///
/// Keys and values are UTF-8 strings. Higher-level helpers like `save_json`
/// and `load_json` are implemented in terms of these primitives.
#[allow(dead_code)]
pub trait StorageBackend: Send + Sync {
    /// Store a string value for a key.
    fn set_string(&self, key: &str, value: &str) -> StorageResult<()>;

    /// Read a string value for a key. Returns Ok(None) when key is missing.
    fn get_string(&self, key: &str) -> StorageResult<Option<String>>;

    /// Remove a key (no-op if key does not exist).
    #[allow(dead_code)]
    fn remove(&self, key: &str) -> StorageResult<()>;

    /// Try to obtain all stored keys (optional optimization).
    #[allow(unused_variables)]
    #[allow(dead_code)]
    fn keys(&self) -> StorageResult<Vec<String>> {
        // Default implementation: not required for all backends.
        Ok(Vec::new())
    }
}

/// NOTE:
/// The generic JSON helpers (`save_json` / `load_json`) were removed from the
/// trait to make it object-safe. Use the free helper functions below when you
/// need to store/load structured data via a `&dyn StorageBackend`.
///
/// Example:
///     save_json_backend(backend.as_ref(), "my-key", &my_struct)?;
///     let v: Option<MyType> = load_json_backend(backend.as_ref(), "my-key")?;
pub fn save_json_backend<T: Serialize>(
    backend: &dyn StorageBackend,
    key: &str,
    value: &T,
) -> StorageResult<()> {
    match serde_json::to_string(value) {
        Ok(s) => backend.set_string(key, &s),
        Err(e) => Err(StorageError::Json(e.to_string())),
    }
}

pub fn load_json_backend<T: DeserializeOwned>(
    backend: &dyn StorageBackend,
    key: &str,
) -> StorageResult<Option<T>> {
    match backend.get_string(key)? {
        Some(s) => match serde_json::from_str::<T>(&s) {
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(StorageError::Json(e.to_string())),
        },
        None => Ok(None),
    }
}

//
// Web implementation (localStorage)
//
#[cfg(target_arch = "wasm32")]
mod web_storage {
    use super::*;
    use wasm_bindgen::JsValue;
    use web_sys::Storage;

    fn local_storage() -> Result<Storage, StorageError> {
        web_sys::window()
            .ok_or_else(|| StorageError::Platform("no window".into()))?
            .local_storage()
            .map_err(|e| StorageError::Platform(format!("local_storage() failed: {:?}", e)))?
            .ok_or_else(|| StorageError::Platform("local_storage not available".into()))
    }

    /// Browser-backed localStorage implementation.
    pub struct WebLocalStorage;

    impl WebLocalStorage {
        pub fn new() -> Self {
            WebLocalStorage {}
        }
    }

    impl StorageBackend for WebLocalStorage {
        fn set_string(&self, key: &str, value: &str) -> StorageResult<()> {
            let storage = local_storage()?;
            storage.set_item(key, value).map_err(|e| {
                StorageError::Platform(format!("set_item error: {:?}", JsValue::from(e)))
            })?;
            Ok(())
        }

        fn get_string(&self, key: &str) -> StorageResult<Option<String>> {
            let storage = local_storage()?;
            match storage.get_item(key) {
                Ok(opt) => Ok(opt),
                Err(e) => Err(StorageError::Platform(format!(
                    "get_item error: {:?}",
                    JsValue::from(e)
                ))),
            }
        }

        fn remove(&self, key: &str) -> StorageResult<()> {
            let storage = local_storage()?;
            storage.remove_item(key).map_err(|e| {
                StorageError::Platform(format!("remove_item error: {:?}", JsValue::from(e)))
            })?;
            Ok(())
        }

        fn keys(&self) -> StorageResult<Vec<String>> {
            let storage = local_storage()?;
            let len = storage.length().map_err(|e| {
                StorageError::Platform(format!(
                    "local_storage length error: {:?}",
                    JsValue::from(e)
                ))
            })?;
            let mut keys = Vec::with_capacity(len as usize);
            for i in 0..len {
                if let Ok(Some(k)) = storage.key(i) {
                    keys.push(k);
                }
            }
            Ok(keys)
        }
    }

    /// Convenience constructor for the default web backend.
    pub fn default_backend() -> Box<dyn StorageBackend> {
        Box::new(WebLocalStorage::new())
    }
}

//
// Native file-backed implementation
//
#[cfg(not(target_arch = "wasm32"))]
mod file_storage {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// File-based storage: stores a single JSON file which is a map of key -> string value.
    ///
    /// Implementation notes:
    /// - On init, file is read into memory (HashMap).
    /// - Mutations update memory and flush the file back to disk synchronously.
    pub struct FileStorage {
        /// Path to the backing JSON file.
        path: PathBuf,
        /// In-memory copy of key -> value
        inner: Mutex<HashMap<String, String>>,
    }

    impl FileStorage {
        /// Determine a good default storage file path for the current user.
        /// Uses environment variables when available:
        /// - On Windows: %APPDATA%/LargeTrackViewer/storage.json
        /// - Else: $HOME/.config/large-track-viewer/storage.json
        fn default_storage_path() -> PathBuf {
            // Prefer APPDATA on Windows
            if cfg!(windows)
                && let Ok(appdata) = std::env::var("APPDATA")
            {
                return Path::new(&appdata)
                    .join("LargeTrackViewer")
                    .join("storage.json");
            }

            if let Ok(home) = std::env::var("HOME") {
                return Path::new(&home)
                    .join(".config")
                    .join("large-track-viewer")
                    .join("storage.json");
            }

            // Fallback to current directory
            Path::new(".").join("large-track-viewer-storage.json")
        }

        pub fn new_with_path(path: Option<PathBuf>) -> Result<Self, StorageError> {
            let path = path.unwrap_or_else(Self::default_storage_path);

            // Ensure parent directory exists
            if let Some(parent) = path.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                return Err(StorageError::Io(format!(
                    "Failed to create storage parent directory: {}",
                    e
                )));
            }

            // Read file if present
            let mut map: HashMap<String, String> = HashMap::new();
            if path.exists() {
                let mut file = fs::File::open(&path)
                    .map_err(|e| StorageError::Io(format!("Failed to open storage file: {}", e)))?;
                let mut s = String::new();
                file.read_to_string(&mut s)
                    .map_err(|e| StorageError::Io(format!("Failed to read storage file: {}", e)))?;
                if !s.trim().is_empty() {
                    match serde_json::from_str::<HashMap<String, String>>(&s) {
                        Ok(m) => map = m,
                        Err(e) => {
                            // If file is corrupted, log and start fresh (avoid panic).
                            return Err(StorageError::Json(format!(
                                "Failed to parse storage JSON: {}",
                                e
                            )));
                        }
                    }
                }
            } else {
                // Ensure file exists by creating empty structure on disk
                let _ = fs::File::create(&path).map_err(|e| {
                    StorageError::Io(format!("Failed to create storage file: {}", e))
                })?;
            }

            Ok(FileStorage {
                path,
                inner: Mutex::new(map),
            })
        }

        fn flush_locked(&self, locked: &HashMap<String, String>) -> StorageResult<()> {
            let s = serde_json::to_string_pretty(locked)
                .map_err(|e| StorageError::Json(e.to_string()))?;
            fs::write(&self.path, s).map_err(|e| StorageError::Io(format!("write failed: {}", e)))
        }
    }

    impl StorageBackend for FileStorage {
        fn set_string(&self, key: &str, value: &str) -> StorageResult<()> {
            let mut guard = self
                .inner
                .lock()
                .map_err(|e| StorageError::Platform(format!("mutex poisoned: {:?}", e)))?;
            guard.insert(key.to_string(), value.to_string());
            self.flush_locked(&guard)
        }

        fn get_string(&self, key: &str) -> StorageResult<Option<String>> {
            let guard = self
                .inner
                .lock()
                .map_err(|e| StorageError::Platform(format!("mutex poisoned: {:?}", e)))?;
            Ok(guard.get(key).cloned())
        }

        fn remove(&self, key: &str) -> StorageResult<()> {
            let mut guard = self
                .inner
                .lock()
                .map_err(|e| StorageError::Platform(format!("mutex poisoned: {:?}", e)))?;
            guard.remove(key);
            self.flush_locked(&guard)
        }

        fn keys(&self) -> StorageResult<Vec<String>> {
            let guard = self
                .inner
                .lock()
                .map_err(|e| StorageError::Platform(format!("mutex poisoned: {:?}", e)))?;
            Ok(guard.keys().cloned().collect())
        }
    }

    pub fn default_backend() -> Result<Box<dyn StorageBackend>, StorageError> {
        Ok(Box::new(FileStorage::new_with_path(None)?))
    }
}

//
// Public helpers to create the default backend for the current platform
//
#[cfg(target_arch = "wasm32")]
pub use web_storage::default_backend as default_storage_backend;

#[cfg(not(target_arch = "wasm32"))]
pub use file_storage::default_backend as default_storage_backend;

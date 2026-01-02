//! Web entry point for egui/eframe applications
//!
//! This module provides a reusable WebHandle for WASM builds.
//!
//! On web, async tasks are executed using tokio-with-wasm which runs
//! them on the JavaScript event loop. This means no manual runtime
//! management is needed - async tasks "just work" when spawned.

use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::prelude::*;

/// Function pointer storage for the app creator.
/// Stored as a usize so we can set it once at runtime.
static APP_CREATOR_PTR: AtomicUsize = AtomicUsize::new(0);

pub fn set_app_creator(creator: fn(&eframe::CreationContext) -> Box<dyn eframe::App>) {
    let ptr = creator as usize;
    match APP_CREATOR_PTR.compare_exchange(0, ptr, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => {
            // Successfully set for the first time.
        }
        Err(existing) => {
            // If it's the same pointer, consider this idempotent and do nothing.
            // If it's different, log a warning and keep the original.
            if existing != ptr {
                tracing::warn!(
                    "app_creator already set to a different function; ignoring subsequent set"
                );
            }
        }
    }
}

fn get_app_creator() -> Option<fn(&eframe::CreationContext) -> Box<dyn eframe::App>> {
    let ptr = APP_CREATOR_PTR.load(Ordering::SeqCst);
    if ptr == 0 {
        None
    } else {
        // SAFETY: we only store function pointers (usize) via set_app_creator,
        // so transmuting back is safe as long as ptr != 0.
        Some(unsafe {
            std::mem::transmute::<usize, fn(&eframe::CreationContext) -> Box<dyn eframe::App>>(ptr)
        })
    }
}

/// Handle to the web app from JavaScript.
#[derive(Clone)]
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
}

#[wasm_bindgen]
impl WebHandle {
    /// Installs a panic hook, then returns.
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen]
    pub fn new() -> Self {
        // XXX: Parse env early
        super::cli::parse_env();
        // Initialize logging for wasm
        {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            use tracing_wasm::WASMLayerConfigBuilder;

            let mut builder = WASMLayerConfigBuilder::new();
            let max_level = if let Some(level_str) = super::cli::get_env::<String>("LOG_LEVEL") {
                match level_str.to_uppercase().as_str() {
                    "TRACE" => tracing::Level::TRACE,
                    "DEBUG" => tracing::Level::DEBUG,
                    "INFO" => tracing::Level::INFO,
                    "WARN" => tracing::Level::WARN,
                    "ERROR" => tracing::Level::ERROR,
                    _ => tracing::Level::INFO,
                }
            } else if cfg!(debug_assertions) {
                tracing::Level::DEBUG
            } else {
                tracing::Level::INFO
            };
            builder.set_max_level(max_level);
            let config = builder.build();
            let _ = tracing_subscriber::registry()
                .with(tracing_wasm::WASMLayer::new(config))
                .try_init();
        }
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));

        Self {
            runner: eframe::WebRunner::new(),
        }
    }

    /// Call this once from JavaScript to start your app.
    #[wasm_bindgen]
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        // Copy the function pointer out so the closure does not capture `&self`,
        // allowing the closure to be 'static as required by WebRunner.
        // get_app_creator now returns an Option to avoid panics if it wasn't set.
        let creator = match get_app_creator() {
            Some(c) => c,
            None => {
                // Return a JS error so the caller can see what went wrong instead of panicking.
                return Err(wasm_bindgen::JsValue::from_str("app_creator not set"));
            }
        };

        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    // On web, tokio-with-wasm handles async tasks on the JS event loop.
                    // No RuntimeAppWrapper needed - async tasks spawned with
                    // tokio_with_wasm::spawn() will be driven by the JS event loop naturally.
                    Ok(creator(cc))
                }),
            )
            .await
    }

    /// Destroys the app and frees resources.
    #[wasm_bindgen]
    pub fn destroy(&self) {
        self.runner.destroy();
    }

    /// The JavaScript can check whether or not your app has crashed.
    #[wasm_bindgen]
    pub fn has_panicked(&self) -> bool {
        self.runner.has_panicked()
    }

    /// Returns the panic message if the app has panicked.
    #[wasm_bindgen]
    pub fn panic_message(&self) -> Option<String> {
        self.runner.panic_summary().map(|s| s.message())
    }

    /// Returns the panic callstack if the app has panicked.
    #[wasm_bindgen]
    pub fn panic_callstack(&self) -> Option<String> {
        self.runner.panic_summary().map(|s| s.callstack())
    }
}

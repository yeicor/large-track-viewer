//! Web entry point for egui/eframe applications
//!
//! This module provides a reusable WebHandle for WASM builds.
//! Applications must define the `create_egui_app` function that this module calls.

use eframe::wasm_bindgen::{self, prelude::*};

// This function must be provided by the application crate
extern "Rust" {
    /// Create the application instance.
    /// This function must be implemented by the application crate using the
    /// `egui_app_creator!` macro or manually.
    fn create_egui_app(cc: &eframe::CreationContext<'_>) -> Box<dyn eframe::App>;
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
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        // Initialize logging for wasm
        #[cfg(feature = "web")]
        {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;

            let _ = tracing_subscriber::registry()
                .with(tracing_wasm::WASMLayer::default())
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
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    // SAFETY: The application crate must provide this function
                    Ok(unsafe { create_egui_app(cc) })
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

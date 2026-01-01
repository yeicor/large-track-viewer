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
    APP_CREATOR_PTR
        .compare_exchange(0, ptr, Ordering::SeqCst, Ordering::SeqCst)
        .expect("app_creator already set");
}

fn get_app_creator() -> fn(&eframe::CreationContext) -> Box<dyn eframe::App> {
    let ptr = APP_CREATOR_PTR.load(Ordering::SeqCst);
    assert!(ptr != 0, "app_creator not set");
    unsafe {
        std::mem::transmute::<usize, fn(&eframe::CreationContext) -> Box<dyn eframe::App>>(ptr)
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
        // Initialize logging for wasm
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
        // Copy the function pointer out so the closure does not capture `&self`,
        // allowing the closure to be 'static as required by WebRunner.
        let creator: fn(&eframe::CreationContext) -> Box<dyn eframe::App> = get_app_creator();

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

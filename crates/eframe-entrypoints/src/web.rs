//! Web entry point for egui/eframe applications
//!
//! This module provides a reusable WebHandle for WASM builds.

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
/// Storage for the tokio runtime handle.
/// Set once at runtime.
static RUNTIME: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();

pub fn set_runtime(handle: tokio::runtime::Handle) {
    RUNTIME.set(handle).expect("runtime already set");
}

fn get_runtime() -> &'static tokio::runtime::Handle {
    RUNTIME.get().expect("runtime not set")
}

use std::time::Duration;

use eframe::{App, Frame, Storage};
use egui::Context;
use egui::RawInput;
use egui::Visuals;

/// A wrapper for eframe::App that ensures all method calls are made within a tokio runtime context.
pub struct RuntimeAppWrapper {
    inner: Box<dyn App>,
}

impl RuntimeAppWrapper {
    /// Creates a new RuntimeAppWrapper wrapping the given app.
    pub fn new(app: Box<dyn App>) -> Self {
        Self { inner: app }
    }
}

fn make_progress(rt: &tokio::runtime::Handle) {
    rt.block_on(async {
        for _i in 0..100000 {
            tokio::task::yield_now().await;
        }
    });
}

impl App for RuntimeAppWrapper {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        let rt = get_runtime();
        let _guard = rt.enter();
        self.inner.update(ctx, frame);
        make_progress(rt);
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        let rt = get_runtime();
        let _guard = rt.enter();
        self.inner.save(storage);
        make_progress(rt);
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        let rt = get_runtime();
        let _guard = rt.enter();
        self.inner.on_exit(gl);
        make_progress(rt);
    }

    fn auto_save_interval(&self) -> Duration {
        let rt = get_runtime();
        let _guard = rt.enter();
        let result = self.inner.auto_save_interval();
        make_progress(rt);
        result
    }

    fn clear_color(&self, visuals: &Visuals) -> [f32; 4] {
        let rt = get_runtime();
        let _guard = rt.enter();
        let result = self.inner.clear_color(visuals);
        make_progress(rt);
        result
    }

    fn persist_egui_memory(&self) -> bool {
        let rt = get_runtime();
        let _guard = rt.enter();
        let result = self.inner.persist_egui_memory();
        make_progress(rt);
        result
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut RawInput) {
        let rt = get_runtime();
        let _guard = rt.enter();
        self.inner.raw_input_hook(ctx, raw_input);
        make_progress(rt);
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
                    let base_app = creator(cc);
                    Ok(Box::new(RuntimeAppWrapper::new(base_app)))
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

//! Generic application runner for egui/eframe applications
//!
//! This module provides generic entry point functions that can be used by any
//! egui/eframe application. For the recommended API, use the `eframe_app!` macro.

/// Setup function for native entry point
///
/// This is a simple wrapper that just returns the app creator.
/// Applications can override this to add custom initialization logic.
#[allow(dead_code)]
pub async fn setup_app<F>(app_creator: F) -> Option<F>
where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
{
    Some(app_creator)
}

#[cfg(not(target_arch = "wasm32"))]
type EventLoopBuilderHook = Option<eframe::EventLoopBuilderHook>;
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
type EventLoopBuilderHook = Option<()>;

/// Native entry point - generic version
///
/// This function can be called by any application to start an eframe app on native platforms.
/// For a cleaner API, consider using the `eframe_app!` macro instead.
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
pub fn native_main_common<F>(
    app_name: &str,
    app_creator: F,
    event_loop_builder: EventLoopBuilderHook,
) where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
{
    // Create a profiling scope marking native main startup so startup time and
    // initialization work are visible in traces. This is a no-op when profiling
    // is disabled.

    use crate::{log_version_info, profiling};
    #[cfg(feature = "profiling")]
    {
        ::profiling::scope!("app::native_main_common");
    }

    // Initialize tracing subscriber with profiling support if enabled
    // This MUST be done before any logging, so both fmt and chrome layers
    // are registered together in the same subscriber
    profiling::setup_logging_and_profiling();

    // Register the main thread name for logging/profiling in debug builds.
    // When the `profiling` feature is enabled we register this thread so that
    // profiler UIs can show a human-friendly thread name in traces.
    #[cfg(feature = "profiling")]
    {
        // Name the main native thread. If your profiling crate uses a function
        // instead of a macro, update this line to call the correct API.
        ::profiling::register_thread!("Main");
    }

    log_version_info();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title(app_name)
            .with_drag_and_drop(true),
        event_loop_builder,
        ..Default::default()
    };

    let _ = eframe::run_native(
        app_name,
        native_options,
        Box::new(move |cc| Ok(app_creator(cc))),
    );
}

/// Desktop entry point with multithreaded runtime
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn desktop_main<F>(app_name: &str, app_creator: F)
where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + Send + 'static,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async { native_main_common(app_name, app_creator, None) });
}

/// Internal implementation for Android entry point.
/// Use the `eframe_app!` macro instead of calling this directly.
///
/// Notes:
/// - When compiled in debug with the `profiling` feature, this function is
///   instrumented and will register the main Android entry thread with the
///   profiling backend so that traces show a meaningful thread name.
/// - The `cfg_attr` ensures the profiling attribute is only applied in
///   debug builds with the `profiling` feature enabled.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub fn android_main(
    app_name: &str,
    app: winit::platform::android::activity::AndroidApp,
    app_creator: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + Send + 'static,
) {
    android_logger::init_once(android_logger::Config::default());
    log::info!("Started logging {} on Android", app_name);

    unsafe {
        // Safe: single-threaded at startup
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        native_main_common(
            app_name,
            app_creator,
            Some(Box::new(|b| {
                use winit::platform::android::EventLoopBuilderExtAndroid;
                b.with_android_app(app);
            })),
        )
    });
}

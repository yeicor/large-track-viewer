//! Generic application runner for egui/eframe applications.
//!
//! This module provides generic entry point functions that can be used by any
//! egui/eframe application. For the recommended API, use the `eframe_app!` macro.

/// Native entry point for desktop and Android.
///
/// On Android, the `AndroidApp` must be set on `NativeOptions.android_app`.
/// On other platforms, this argument is ignored.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_main<F>(
    app_name: &str,
    app_creator: F,
    #[cfg(target_os = "android")] android_app: winit::platform::android::activity::AndroidApp,
) where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
{
    #[cfg(feature = "profiling")]
    ::profiling::scope!("app::native_main");
    #[cfg(feature = "profiling")]
    ::profiling::register_thread!("Main");

    // Initialize tracing/profiling except on Android (where android_logger is used)
    crate::profiling::setup_logging_and_profiling();

    crate::log_version_info();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title(app_name)
            .with_drag_and_drop(true),
        #[cfg(target_os = "android")]
        android_app: Some(android_app),
        ..Default::default()
    };

    let _ = eframe::run_native(
        app_name,
        native_options,
        Box::new(move |cc| Ok(app_creator(cc))),
    );
}

/// Desktop entry point with multithreaded runtime (not used on Android).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn desktop_main<F>(app_name: &str, app_creator: F)
where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + Send + 'static,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();
    native_main(app_name, app_creator);
}

/// Android entry point.
/// This is called from the macro-generated `android_main` function.
#[cfg(target_os = "android")]
pub fn android_main(
    app_name: &str,
    app: winit::platform::android::activity::AndroidApp,
    app_creator: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + Send + 'static,
) {
    /*android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("LargeTrackViewer"),
    );*/
    unsafe {
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    // Ensure a tokio runtime is available for async tasks.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    #[cfg(target_os = "android")]
    {
        *crate::file_picker::ANDROID_APP.lock().unwrap() = Some(app.clone());
    }
    native_main(app_name, move |cc| app_creator(cc), app);
}

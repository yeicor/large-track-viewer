// === Entry point for android ===
#[cfg(target_os = "android")]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default());
    log::info!("Starting Large Track Viewer on Android");

    unsafe {
        // Safe: single-threaded at startup
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        if let Some(app_creator) = super::run::setup_app().await {
            let native_options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_title("Large Track Viewer"),
                event_loop_builder: Some(Box::new(move |builder| {
                    builder.with_android_app(app);
                })),
                ..Default::default()
            };

            let _ = eframe::run_native(
                "Large Track Viewer",
                native_options,
                Box::new(move |cc| Ok(app_creator(cc))),
            );
        }
    });
}

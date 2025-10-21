mod app;
mod entrypoints;

// Entry point for Android
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    entrypoints::lib::android_main(app);
}

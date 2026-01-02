use shadow_rs::shadow;

shadow!(build);

/// Log version info using the appropriate logging mechanism for the platform.
/// On Android, we use the `log` crate (which android_logger handles).
/// On other platforms, we use `tracing` (which our tracing_subscriber handles).
#[allow(dead_code)] // Allow auto-generated code containing unused build metadata
pub fn log_version_info() {
    #[cfg(target_os = "android")]
    {
        log::info!("{}", short_version_info());
        log::info!(
            "Build date: {} ({})",
            build::BUILD_TIME_2822,
            build::BUILD_RUST_CHANNEL
        );
    }
    #[cfg(not(target_os = "android"))]
    {
        tracing::info!("{}", short_version_info());
        tracing::info!(
            "Build date: {} ({})",
            build::BUILD_TIME_2822,
            build::BUILD_RUST_CHANNEL
        );
    }
}
#[allow(dead_code)] // Allow auto-generated code containing unused build metadata
pub fn short_version_info() -> String {
    use std::path::Path;

    let project_name = Path::new(&build::CARGO_MANIFEST_DIR)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown_project");

    format!(
        "{} {} ({}@{}{})",
        project_name,
        build::PKG_VERSION,
        build::BRANCH,
        build::SHORT_COMMIT,
        if build::GIT_CLEAN { "" } else { "+dirty" }
    )
}

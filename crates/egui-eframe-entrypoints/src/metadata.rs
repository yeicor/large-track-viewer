use shadow_rs::shadow;
use tracing::info;

shadow!(build);

#[allow(dead_code)] // Allow auto-generated code containing unused build metadata
pub fn log_version_info() {
    info!("{}", short_version_info());
    info!(
        "Build date: {} ({})",
        build::BUILD_TIME_2822,
        build::BUILD_RUST_CHANNEL
    );
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

use super::metadata::log_version_info;
use crate::{app::cli::Cli, entrypoints::cli::parse_args};

/// Setup and create the app
#[allow(dead_code)]
pub async fn setup_app()
-> Option<Box<dyn FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>>> {
    log_version_info();
    let cli_args = match parse_args::<Cli>() {
        Ok(args) => args,
        Err(e) => {
            #[cfg(not(target_arch = "wasm32"))]
            e.exit();
            #[cfg(target_arch = "wasm32")]
            {
                let user_msg = format!(
                    "Error parsing CLI:\n{}\n
You should change the GET params, using the cli prefix.\n
Starting anyway without args.",
                    e
                );
                if let Some(window) = web_sys::window() {
                    window.alert_with_message(&user_msg).unwrap_or(());
                } else {
                    tracing::error!(user_msg);
                }
                use clap::Parser;
                Cli::parse_from(Vec::<String>::new()) // Default args on web if parsing fails
            }
        }
    };
    Some(Box::new(|cc| {
        Box::new(crate::app::LargeTrackViewerApp::new(cli_args, cc))
    }))
}

/// Native entry point
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
pub async fn native_main() {
    // Setup logging
    tracing_subscriber::fmt::init();

    if let Some(app_creator) = setup_app().await {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 720.0])
                .with_title("Large Track Viewer"),
            ..Default::default()
        };

        let _ = eframe::run_native(
            "Large Track Viewer",
            native_options,
            Box::new(move |cc| Ok(app_creator(cc))),
        );
    }
}

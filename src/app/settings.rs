use crate::entrypoints::cli::parse_args;
use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
/// Large Track Viewer - A cross-platform application for viewing and analyzing large GPS tracks
pub struct Settings {
    // Add command-line arguments here as needed
}

impl Settings {
    /// Create default settings
    pub fn from_cli() -> Self {
        match parse_args::<Settings>() {
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
                    Settings::parse_from(Vec::<String>::new()) // Default args on web if parsing fails
                }
            }
        }
    }
}

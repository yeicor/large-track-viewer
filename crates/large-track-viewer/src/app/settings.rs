use clap::Parser;
use eframe_entrypoints::parse_args;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
/// Large Track Viewer - A cross-platform application for viewing and analyzing large GPS tracks
pub struct Settings {
    /// GPX files to load on startup
    #[clap(short, long, value_name = "FILE")]
    pub gpx_files: Vec<PathBuf>,

    /// LOD bias (higher = more detail, range: 0.001-1000)
    #[clap(short, long, default_value = "1.0")]
    pub bias: f64,

    /// Maximum points per quadtree node before subdivision
    #[clap(long, default_value = "100")]
    pub max_points_per_node: usize,

    /// Reference viewport width in pixels for LOD calculations
    #[clap(long, default_value = "1920")]
    pub reference_viewport_width: u32,

    /// Reference viewport height in pixels for LOD calculations
    #[clap(long, default_value = "1080")]
    pub reference_viewport_height: u32,

    /// Track line width in pixels
    #[clap(long, default_value = "2.0")]
    pub line_width: f32,

    /// Show outline/border around tracks for better visibility
    #[clap(long, default_value = "true")]
    pub show_outline: bool,

    /// Ignore previously persisted state and start fresh
    #[clap(long, default_value = "false")]
    pub ignore_persisted: bool,
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

use clap::Parser;
use egui_eframe_entrypoints::parse_args;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
/// Large Track Viewer - A cross-platform application for viewing and analyzing large GPS tracks
pub struct Settings {
    /// GPX files to load on startup
    #[clap(short, long, value_name = "FILE")]
    pub gpx_files: Vec<PathBuf>,

    /// LOD bias (higher = more detail, typical range: 0.1-10.0)
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

    /// Initial map center latitude (WGS84)
    #[clap(long)]
    pub center_lat: Option<f64>,

    /// Initial map center longitude (WGS84)
    #[clap(long)]
    pub center_lon: Option<f64>,

    /// Initial map zoom level
    #[clap(long, default_value = "12")]
    pub zoom: u8,

    /// Track line width in pixels
    #[clap(long, default_value = "2.0")]
    pub line_width: f32,

    /// Track color (hex format, e.g., FF0000 for red)
    #[clap(long, default_value = "0000FF")]
    pub track_color: String,
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

    /// Parse hex color string to RGB
    pub fn parse_track_color(&self) -> egui::Color32 {
        if let Ok(rgb) = u32::from_str_radix(&self.track_color, 16) {
            egui::Color32::from_rgb(
                ((rgb >> 16) & 0xFF) as u8,
                ((rgb >> 8) & 0xFF) as u8,
                (rgb & 0xFF) as u8,
            )
        } else {
            egui::Color32::BLUE // Default fallback
        }
    }

    /// Get initial map position if specified
    #[allow(dead_code)] // Will be used when MapMemory API allows setting initial position
    pub fn get_initial_position(&self) -> Option<walkers::Position> {
        self.center_lat
            .and_then(|lat| self.center_lon.map(|lon| walkers::lat_lon(lat, lon)))
    }
}

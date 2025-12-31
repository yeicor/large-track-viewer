//! Walkers plugin for integrating GPX route rendering with the map view
//!
//! This module provides a custom walkers plugin that queries visible route segments
//! from the data module and renders them on the map with proper LOD handling.

use egui::{Color32, Stroke};
use large_track_lib::{RouteCollection, SimplifiedSegment};
use std::sync::Arc;
use tokio::sync::RwLock;
use walkers::{Plugin, Projector};

/// Statistics from the last render pass
#[derive(Default, Clone, Debug)]
pub struct RenderStats {
    /// Number of segments rendered
    pub segments_rendered: usize,
    /// Number of simplified points rendered (actual points drawn)
    pub simplified_points_rendered: usize,
}

/// Plugin for rendering GPX tracks on the map
pub struct TrackPlugin {
    /// Reference to the route collection
    collection: Arc<RwLock<RouteCollection>>,
    /// Line width for rendering tracks
    width: f32,
    /// Whether to show outline/border around tracks
    show_outline: bool,
    /// Shared statistics output (updated after each render)
    stats: Arc<RwLock<RenderStats>>,
}

impl TrackPlugin {
    /// Create a new track plugin with a shared stats output
    pub fn new(
        collection: Arc<RwLock<RouteCollection>>,
        width: f32,
        show_outline: bool,
        stats: Arc<RwLock<RenderStats>>,
    ) -> Self {
        Self {
            collection,
            width,
            show_outline,
            stats,
        }
    }

    /// Generate a color for a route based on its index
    fn get_route_color(route_id: usize) -> Color32 {
        // Use golden angle for good color distribution
        let hue = (route_id as f32 * 137.508) % 360.0;
        let saturation = 0.75;
        let value = 0.85;

        // Convert HSV to RGB
        let c = value * saturation;
        let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
        let m = value - c;

        let (r, g, b) = if hue < 60.0 {
            (c, x, 0.0)
        } else if hue < 120.0 {
            (x, c, 0.0)
        } else if hue < 180.0 {
            (0.0, c, x)
        } else if hue < 240.0 {
            (0.0, x, c)
        } else if hue < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Color32::from_rgb(
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
        )
    }

    /// Render a single simplified segment and return the number of points drawn
    fn render_segment(
        &self,
        segment: &SimplifiedSegment,
        projector: &Projector,
        painter: &egui::Painter,
    ) -> usize {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let name = segment
            .route
            .gpx_data()
            .metadata
            .as_ref()
            .and_then(|m| m.name.as_deref())
            .unwrap_or("");
        hasher.write(name.as_bytes());
        let color = Self::get_route_color(hasher.finish() as usize);

        // Inner stroke with the route color
        let inner_stroke = Stroke::new(self.width, color);
        // Outer stroke (dark outline) for better visibility - only used if show_outline is true
        let outline_stroke = Stroke::new(self.width + 2.0, Color32::from_black_alpha(180));

        let mut points_drawn = 0;

        for part in &segment.parts {
            let points = part.get_simplified_points(&segment.route);

            if points.is_empty() {
                continue;
            }

            // Convert WGS84 coordinates to screen space
            let screen_points: Vec<egui::Pos2> = points
                .iter()
                .map(|waypoint| {
                    let point = waypoint.point();
                    let position = walkers::lat_lon(point.y(), point.x());
                    let screen_vec = projector.project(position);
                    egui::Pos2::new(screen_vec.x, screen_vec.y)
                })
                .collect();

            // Draw the polyline if we have at least 2 points
            if screen_points.len() >= 2 {
                points_drawn += screen_points.len();

                if self.show_outline {
                    // Draw outline first (underneath)
                    painter.add(egui::Shape::line(screen_points.clone(), outline_stroke));
                }
                // Draw colored line on top
                painter.add(egui::Shape::line(screen_points, inner_stroke));
            }
        }

        points_drawn
    }
}

impl Plugin for TrackPlugin {
    fn run(
        self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &Projector,
        _map_memory: &walkers::MapMemory,
    ) {
        profiling::scope!("TrackPlugin::run");

        let painter = ui.painter();

        // Get the viewport bounds in screen space
        let viewport_rect = response.rect;

        // Convert screen corners to geographic positions and then to Web Mercator
        let top_left_pos =
            projector.unproject(egui::Vec2::new(viewport_rect.min.x, viewport_rect.min.y));
        let bottom_right_pos =
            projector.unproject(egui::Vec2::new(viewport_rect.max.x, viewport_rect.max.y));

        {
            // Convert to Web Mercator coordinates for querying
            let min_mercator = large_track_lib::utils::wgs84_to_mercator(
                top_left_pos.y().min(bottom_right_pos.y()),
                top_left_pos.x().min(bottom_right_pos.x()),
            );
            let max_mercator = large_track_lib::utils::wgs84_to_mercator(
                top_left_pos.y().max(bottom_right_pos.y()),
                top_left_pos.x().max(bottom_right_pos.x()),
            );

            // Create viewport rectangle in Web Mercator space
            let viewport = geo::Rect::new(
                geo::Coord {
                    x: min_mercator.x(),
                    y: min_mercator.y(),
                },
                geo::Coord {
                    x: max_mercator.x(),
                    y: max_mercator.y(),
                },
            );

            // Query visible segments from the collection
            // Pass screen size for dynamic LOD adjustment
            let screen_size = (viewport_rect.width() as f64, viewport_rect.height() as f64);
            let segments: Vec<SimplifiedSegment> = {
                profiling::scope!("query_visible");
                if let Ok(collection) = self.collection.try_read() {
                    collection.query_visible(viewport, screen_size)
                } else {
                    Vec::new()
                }
            };

            // Render all visible segments and count points
            let mut total_points = 0;
            {
                profiling::scope!("render_segments");
                for segment in &segments {
                    total_points += self.render_segment(segment, projector, painter);
                }
            }

            // Update shared statistics
            {
                if let Ok(mut stats) = self.stats.try_write() {
                    stats.segments_rendered = segments.len();
                    stats.simplified_points_rendered = total_points;
                }
            }
        }
    }
}

//! Walkers plugin for integrating GPX route rendering with the map view
//!
//! This module provides a custom walkers plugin that queries visible route segments
//! from the data module and renders them on the map with proper LOD handling.

use egui::{Color32, Stroke};
use large_track_lib::{RouteCollection, SimplifiedSegment, utils};
use walkers::{Plugin, Projector};

/// Plugin for rendering GPX tracks on the map
pub struct TrackPlugin {
    /// Reference to the route collection
    collection: std::sync::Arc<std::sync::RwLock<RouteCollection>>,
    /// Line width for rendering tracks
    width: f32,
    /// Whether to show boundary context (smoother rendering at viewport edges)
    show_boundary_context: bool,
}

impl TrackPlugin {
    /// Create a new track plugin
    pub fn new(collection: std::sync::Arc<std::sync::RwLock<RouteCollection>>, width: f32) -> Self {
        Self {
            collection,
            width,
            show_boundary_context: true,
        }
    }

    /// Set whether to show boundary context
    pub fn with_boundary_context(mut self, enabled: bool) -> Self {
        self.show_boundary_context = enabled;
        self
    }

    /// Set the line width
    #[allow(dead_code)] // May be used for dynamic width changes
    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    /// Render a single simplified segment
    fn render_segment(
        &self,
        segment: &SimplifiedSegment,
        projector: &Projector,
        painter: &egui::Painter,
    ) {
        // Use fixed color for now (blue) - TODO: per-track colors
        let color = Color32::from_rgb(70, 130, 220);
        let stroke = Stroke::new(self.width, color);

        for part in &segment.parts {
            let points = if self.show_boundary_context {
                part.get_points_with_context(&segment.route)
            } else {
                part.get_simplified_points(&segment.route)
            };

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
                painter.add(egui::Shape::line(screen_points, stroke));
            }
        }
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
            let min_mercator = utils::wgs84_to_mercator(
                top_left_pos.y().min(bottom_right_pos.y()),
                top_left_pos.x().min(bottom_right_pos.x()),
            );
            let max_mercator = utils::wgs84_to_mercator(
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
            // Clone the segments to owned data to avoid lifetime issues
            let segments: Vec<SimplifiedSegment> = {
                profiling::scope!("query_visible");
                let collection = self.collection.read().unwrap();
                collection
                    .query_visible(viewport)
                    .into_iter()
                    .cloned()
                    .collect()
            };

            // Render all visible segments
            profiling::scope!("render_segments");
            for segment in &segments {
                self.render_segment(segment, projector, painter);
            }
        }
    }
}

/// Helper to create boundary context markers for debugging
pub struct BoundaryContextPlugin {
    collection: std::sync::Arc<std::sync::RwLock<RouteCollection>>,
}

impl BoundaryContextPlugin {
    #[allow(dead_code)]
    pub fn new(collection: std::sync::Arc<std::sync::RwLock<RouteCollection>>) -> Self {
        Self { collection }
    }
}

impl Plugin for BoundaryContextPlugin {
    fn run(
        self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &Projector,
        _map_memory: &walkers::MapMemory,
    ) {
        profiling::scope!("BoundaryContextPlugin::run");

        let painter = ui.painter();

        let viewport_rect = response.rect;
        let top_left_pos =
            projector.unproject(egui::Vec2::new(viewport_rect.min.x, viewport_rect.min.y));
        let bottom_right_pos =
            projector.unproject(egui::Vec2::new(viewport_rect.max.x, viewport_rect.max.y));

        {
            let min_mercator = utils::wgs84_to_mercator(
                top_left_pos.y().min(bottom_right_pos.y()),
                top_left_pos.x().min(bottom_right_pos.x()),
            );
            let max_mercator = utils::wgs84_to_mercator(
                top_left_pos.y().max(bottom_right_pos.y()),
                top_left_pos.x().max(bottom_right_pos.x()),
            );

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

            let segments: Vec<SimplifiedSegment> = {
                let collection = self.collection.read().unwrap();
                collection
                    .query_visible(viewport)
                    .into_iter()
                    .cloned()
                    .collect()
            };

            // Draw small circles at boundary context points
            for segment in &segments {
                for part in &segment.parts {
                    // Draw prev context point in green
                    if let Some(prev) = part.get_prev_point(&segment.route) {
                        let point = prev.point();
                        let pos = walkers::lat_lon(point.y(), point.x());
                        let screen_vec = projector.project(pos);
                        let screen_pos = egui::Pos2::new(screen_vec.x, screen_vec.y);
                        painter.circle_filled(screen_pos, 3.0, Color32::GREEN);
                    }

                    // Draw next context point in red
                    if let Some(next) = part.get_next_point(&segment.route) {
                        let point = next.point();
                        let pos = walkers::lat_lon(point.y(), point.x());
                        let screen_vec = projector.project(pos);
                        let screen_pos = egui::Pos2::new(screen_vec.x, screen_vec.y);
                        painter.circle_filled(screen_pos, 3.0, Color32::RED);
                    }
                }
            }
        }
    }
}

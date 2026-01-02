//! Walkers plugin for integrating GPX route rendering with the map view
//!
//! This module provides a custom walkers plugin that queries visible route segments
//! from the data module and renders them on the map with proper LOD handling.

use eframe_entrypoints::async_runtime::RwLock;
use egui::{Color32, Stroke};
use large_track_lib::{RouteCollection, SimplifiedSegment};
use std::sync::Arc;
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
    /// Shared selected route handle (owned by AppState). Use async RwLock for cross-platform compatibility.
    selected: Arc<RwLock<Option<usize>>>,
}

impl TrackPlugin {
    /// Create a new track plugin with a shared stats output and a shared selection handle
    pub fn new(
        collection: Arc<RwLock<RouteCollection>>,
        width: f32,
        show_outline: bool,
        stats: Arc<RwLock<RenderStats>>,
        selected: Arc<RwLock<Option<usize>>>,
    ) -> Self {
        Self {
            collection,
            width,
            show_outline,
            stats,
            selected,
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
        #[cfg(feature = "profiling")]
        profiling::scope!("plugin::render_segment");
        // Use route_index as a stable, cheap color seed (avoids hashing metadata string)
        let color = Self::get_route_color(segment.route_index);

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
            // Pre-allocate to avoid repeated allocations during mapping
            let mut screen_points: Vec<egui::Pos2> = Vec::with_capacity(points.len());
            for waypoint in points {
                let point = waypoint.point();
                let position = walkers::lat_lon(point.y(), point.x());
                let screen_vec = projector.project(position);
                screen_points.push(egui::Pos2::new(screen_vec.x, screen_vec.y));
            }

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

    /// Render a segment using an explicit highlight color/stroke (used for selected route)
    fn render_segment_highlight(
        &self,
        segment: &SimplifiedSegment,
        projector: &Projector,
        painter: &egui::Painter,
    ) {
        #[cfg(feature = "profiling")]
        profiling::scope!("plugin::render_segment_highlight");
        let highlight_color = Color32::from_rgb(255, 200, 0);
        let highlight_stroke = Stroke::new(self.width + 3.0, highlight_color);
        let outline_stroke = Stroke::new(self.width + 5.0, Color32::from_black_alpha(200));

        for part in &segment.parts {
            let points = part.get_points_with_context(&segment.route);

            if points.is_empty() {
                continue;
            }

            // Pre-allocate screen_points to avoid allocation churn during rendering
            let mut screen_points: Vec<egui::Pos2> = Vec::with_capacity(points.len());
            for waypoint in points {
                let point = waypoint.point();
                let position = walkers::lat_lon(point.y(), point.x());
                let screen_vec = projector.project(position);
                screen_points.push(egui::Pos2::new(screen_vec.x, screen_vec.y));
            }

            if screen_points.len() >= 2 {
                if self.show_outline {
                    painter.add(egui::Shape::line(screen_points.clone(), outline_stroke));
                }
                painter.add(egui::Shape::line(screen_points, highlight_stroke));
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
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Block briefly on native to ensure we get a consistent result
                    eframe_entrypoints::async_runtime::blocking_read(
                        &self.collection,
                        |collection| collection.query_visible(viewport, screen_size),
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // On web avoid blocking the main thread; fall back to try_read.
                    if let Ok(collection) = self.collection.try_read() {
                        collection.query_visible(viewport, screen_size)
                    } else {
                        Vec::new()
                    }
                }
            };

            // Handle map click to select nearest route.
            // If the map area was clicked, find nearest visible route (by projected screen distance)
            if response.clicked() {
                // Retrieve the pointer position via the UI context (safe and available here).
                if let Some(click_pos) = ui.ctx().input(|i| i.pointer.interact_pos()) {
                    // Convert click to geographic and mercator
                    let click_geo = projector.unproject(egui::Vec2::new(click_pos.x, click_pos.y));
                    let click_merc =
                        large_track_lib::utils::wgs84_to_mercator(click_geo.y(), click_geo.x());

                    // Build a small query window around click (meters)
                    let radius_m = 1000.0; // 1km search radius in mercator meters
                    let query_rect = geo::Rect::new(
                        geo::Coord {
                            x: click_merc.x() - radius_m,
                            y: click_merc.y() - radius_m,
                        },
                        geo::Coord {
                            x: click_merc.x() + radius_m,
                            y: click_merc.y() + radius_m,
                        },
                    );

                    // Query segments near click (use same screen_size)
                    #[cfg(not(target_arch = "wasm32"))]
                    let nearby_segments = eframe_entrypoints::async_runtime::blocking_read(
                        &self.collection,
                        |collection| collection.query_visible(query_rect, screen_size),
                    );
                    #[cfg(target_arch = "wasm32")]
                    let nearby_segments = if let Ok(collection) = self.collection.try_read() {
                        collection.query_visible(query_rect, screen_size)
                    } else {
                        Vec::new()
                    };

                    // Compute nearest route among nearby_segments by screen-space distance
                    let mut best: Option<(usize, f32)> = None; // (route_index, distance_pixels)
                    for seg in &nearby_segments {
                        for part in &seg.parts {
                            // Use points with context for better hit testing
                            let waypoints = part.get_points_with_context(&seg.route);
                            for wp in waypoints {
                                let p = wp.point();
                                let pos = walkers::lat_lon(p.y(), p.x());
                                let sv = projector.project(pos);
                                let sp = egui::pos2(sv.x, sv.y);
                                let dx = sp.x - click_pos.x;
                                let dy = sp.y - click_pos.y;
                                let dist = (dx * dx + dy * dy).sqrt();
                                let entry = (seg.route_index, dist);
                                match best {
                                    Some((_, best_d)) => {
                                        if dist < best_d {
                                            best = Some(entry);
                                        }
                                    }
                                    None => best = Some(entry),
                                }
                            }
                        }
                    }

                    // Threshold (pixels) to consider a click a hit
                    let hit_threshold = 12.0;
                    if let Some((route_idx, dist)) = best {
                        if dist <= hit_threshold {
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                eframe_entrypoints::async_runtime::blocking_write(
                                    &self.selected,
                                    |g| *g = Some(route_idx),
                                );
                            }
                            #[cfg(target_arch = "wasm32")]
                            {
                                if let Ok(mut guard) = self.selected.try_write() {
                                    *guard = Some(route_idx);
                                }
                            }
                        } else {
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                eframe_entrypoints::async_runtime::blocking_write(
                                    &self.selected,
                                    |g| *g = None,
                                );
                            }
                            #[cfg(target_arch = "wasm32")]
                            {
                                if let Ok(mut guard) = self.selected.try_write() {
                                    *guard = None;
                                }
                            }
                        }
                    } else {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            eframe_entrypoints::async_runtime::blocking_write(
                                &self.selected,
                                |g| *g = None,
                            );
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            if let Ok(mut guard) = self.selected.try_write() {
                                *guard = None;
                            }
                        }
                    }
                }
            }

            // Render all visible segments and count points.
            // We render non-selected routes first, then selected route(s) on top.
            let mut total_points = 0usize;
            {
                profiling::scope!("render_segments");

                let selected = {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let mut tmp: Option<usize> = None;
                        eframe_entrypoints::async_runtime::blocking_read(&self.selected, |g| {
                            tmp = *g;
                        });
                        tmp
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        if let Ok(guard) = self.selected.try_read() {
                            *guard
                        } else {
                            None
                        }
                    }
                };

                // First pass: non-selected
                for segment in &segments {
                    if Some(segment.route_index) == selected {
                        continue;
                    }
                    total_points += self.render_segment(segment, projector, painter);
                }

                // Second pass: selected route(s) drawn on top with highlight
                if let Some(sel_idx) = selected {
                    for segment in &segments {
                        if segment.route_index == sel_idx {
                            // count points using the regular renderer for stats, but draw highlight
                            // We'll count simplified points for stats
                            for part in &segment.parts {
                                let pts = part.get_simplified_points(&segment.route);
                                total_points += pts.len();
                            }
                            self.render_segment_highlight(segment, projector, painter);
                        }
                    }
                }
            }

            // Update shared statistics
            {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    eframe_entrypoints::async_runtime::blocking_write(&self.stats, |s| {
                        s.segments_rendered = segments.len();
                        s.simplified_points_rendered = total_points;
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    if let Ok(mut stats) = self.stats.try_write() {
                        stats.segments_rendered = segments.len();
                        stats.simplified_points_rendered = total_points;
                    }
                }
            }
        }
    }
}

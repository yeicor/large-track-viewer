//! RouteCollection - Top-level manager for routes, quadtree, and queries
//!
//! This module provides the high-level API for managing multiple GPX routes,
//! building spatial indices, and executing viewport queries.

use crate::{Quadtree, Result, Route, SimplifiedSegment, utils};

use geo::Rect;
use rayon::prelude::*;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// Configuration for the route collection
///
/// The LOD (Level of Detail) system automatically adjusts simplification based on the
/// screen size passed to `query_visible()`. This ensures consistent visual quality
/// across different screen resolutions without needing to reconfigure the collection.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Config {
    /// Reference pixel viewport used as a baseline for LOD calculations.
    /// The actual screen size is passed at query time, and the LOD tolerance
    /// is scaled based on the ratio of actual screen size to this reference.
    /// Default: 1024x768
    pub reference_pixel_viewport: Rect<f64>,
    /// LOD bias factor (default 1.0).
    /// Higher values retain more detail (lower simplification tolerance).
    /// Lower values simplify more aggressively for better performance.
    /// A bias of 1.0 targets approximately 1 pixel minimum feature size.
    pub bias: f64,
    /// Subdivision threshold for quadtree nodes (currently unused, reserved for future use)
    pub max_points_per_node: usize,
}

#[cfg_attr(feature = "profiling", profiling::all_functions)]
impl Default for Config {
    fn default() -> Self {
        Self {
            reference_pixel_viewport: Rect::new(
                geo::Coord { x: 0.0, y: 0.0 },
                geo::Coord {
                    x: 1024.0,
                    y: 768.0,
                },
            ),
            bias: 1.0,
            max_points_per_node: 100,
        }
    }
}

/// Information about the route collection
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CollectionInfo {
    /// Number of routes loaded
    pub route_count: usize,
    /// Total number of track points
    pub total_points: usize,
    /// Total distance in meters
    pub total_distance_meters: f64,
}

/// Cached statistics for the collection
///
/// These are updated incrementally when routes are added or removed,
/// avoiding expensive recalculation.
#[derive(Debug, Clone, Default)]
struct CachedStats {
    /// Total number of points across all routes
    total_points: usize,
    /// Total distance in meters across all routes
    total_distance: f64,
    /// Cached bounding box in Web Mercator (None if empty)
    bounding_box_mercator: Option<Rect<f64>>,
}

/// Top-level manager for all routes and queries
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RouteCollection {
    /// All loaded routes
    routes: Vec<Arc<Route>>,
    /// Spatial index for fast queries
    quadtree: Quadtree,
    /// Configuration settings
    config: Config,
    /// Cached statistics (incrementally updated)
    #[cfg_attr(feature = "serde", serde(skip, default))]
    cached_stats: CachedStats,
}

#[cfg_attr(feature = "profiling", profiling::all_functions)]
impl RouteCollection {
    /// Create a new route collection with the given configuration
    pub fn new(config: Config) -> Self {
        // Profile high-level construction of the collection.
        #[cfg(feature = "profiling")]
        profiling::scope!("collection::new");

        let quadtree = Quadtree::new(config.reference_pixel_viewport, config.bias);
        Self {
            routes: Vec::new(),
            quadtree,
            config,
            cached_stats: CachedStats::default(),
        }
    }

    /// Add a route to the collection
    ///
    /// Parses the GPX data, builds a quadtree for the route, and merges it
    /// into the main spatial index.
    pub fn add_route(&mut self, gpx_data: gpx::Gpx) -> Result<()> {
        // Profile single-route addition (parsing, quadtree build, merge)
        #[cfg(feature = "profiling")]
        profiling::scope!("collection::add_route");

        let route = Route::new(gpx_data)?;
        let route_index = self.routes.len();

        // Build quadtree for this route
        let route_quadtree = Quadtree::new_with_route(
            route.clone(),
            route_index,
            self.config.reference_pixel_viewport,
            self.config.bias,
        )?;

        // Merge into main quadtree
        self.quadtree.merge(route_quadtree)?;

        // Update cached statistics incrementally
        self.update_stats_for_added_route(&route);

        // Store route reference
        self.routes.push(route);

        Ok(())
    }

    /// Add multiple routes in parallel
    ///
    /// This is more efficient than adding routes one by one as it parallelizes
    /// both parsing and quadtree construction.
    pub fn add_routes_parallel(&mut self, gpx_data_vec: Vec<gpx::Gpx>) -> Result<()> {
        // Profile parallel route ingestion (parsing + quadtree construction + merge)
        #[cfg(feature = "profiling")]
        profiling::scope!("collection::add_routes_parallel");

        let start_index = self.routes.len();

        // Parse and build quadtrees in parallel
        let results: Result<Vec<(Arc<Route>, Quadtree)>> = gpx_data_vec
            .into_par_iter()
            .enumerate()
            .map(|(i, gpx_data)| {
                let route = Route::new(gpx_data)?;
                let route_index = start_index + i;
                let quadtree = Quadtree::new_with_route(
                    route.clone(),
                    route_index,
                    self.config.reference_pixel_viewport,
                    self.config.bias,
                )?;
                Ok((route, quadtree))
            })
            .collect();

        let route_quadtrees = results?;

        // Sequential merge (fast due to structural alignment)
        for (route, quadtree) in route_quadtrees {
            self.quadtree.merge(quadtree)?;
            // Update cached statistics incrementally
            self.update_stats_for_added_route(&route);
            self.routes.push(route);
        }

        Ok(())
    }

    /// Load routes from GPX files in parallel
    pub fn load_from_files<P: AsRef<Path> + Send + Sync>(&mut self, paths: Vec<P>) -> Result<()> {
        // Profile bulk file loading (IO + parsing + parallel route build)
        #[cfg(feature = "profiling")]
        profiling::scope!("collection::load_from_files");

        let gpx_data_vec: Result<Vec<gpx::Gpx>> = paths
            .into_par_iter()
            .map(|path| {
                let file = std::fs::File::open(path.as_ref())?;
                let reader = std::io::BufReader::new(file);
                Ok(gpx::read(reader)?)
            })
            .collect();

        self.add_routes_parallel(gpx_data_vec?)
    }

    /// Query for visible segments in the given viewport
    ///
    /// The viewport should be in Web Mercator coordinates (EPSG:3857).
    /// Returns segments at the appropriate LOD level for the viewport size.
    /// Simplification is performed lazily and cached for efficiency.
    ///
    /// # Arguments
    /// * `geo_viewport` - The geographic viewport in Web Mercator coordinates
    /// * `screen_size` - Current screen size (width, height) in pixels.
    ///   The LOD tolerance is automatically adjusted based on screen size,
    ///   ensuring consistent visual quality across different screen resolutions.
    ///   A bias of 1.0 will produce similar visual results regardless of screen resolution.
    ///
    /// # Example
    /// ```ignore
    /// // Query with actual screen dimensions
    /// let segments = collection.query_visible(viewport, (1920.0, 1080.0));
    ///
    /// // For 4K displays, more detail will be preserved automatically
    /// let segments = collection.query_visible(viewport, (3840.0, 2160.0));
    /// ```
    #[inline]
    pub fn query_visible(
        &self,
        geo_viewport: Rect<f64>,
        screen_size: (f64, f64),
    ) -> Vec<SimplifiedSegment> {
        // Top-level collection query scope to attribute overhead outside the quadtree internals
        #[cfg(feature = "profiling")]
        profiling::scope!("collection::query_visible");

        self.quadtree.query(geo_viewport, screen_size)
    }

    /// Get total number of routes
    #[inline]
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Get total number of points across all routes
    ///
    /// This is O(1) as the value is cached and updated incrementally.
    #[inline]
    pub fn total_points(&self) -> usize {
        self.cached_stats.total_points
    }

    /// Get total distance across all routes in meters
    ///
    /// This is O(1) as the value is cached and updated incrementally.
    #[inline]
    pub fn total_distance(&self) -> f64 {
        self.cached_stats.total_distance
    }

    /// Get collection information
    ///
    /// This is O(1) as all values are cached.
    #[inline]
    pub fn get_info(&self) -> CollectionInfo {
        CollectionInfo {
            route_count: self.routes.len(),
            total_points: self.cached_stats.total_points,
            total_distance_meters: self.cached_stats.total_distance,
        }
    }

    /// Get a reference to the configuration
    #[inline]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get a reference to a specific route by index
    #[inline]
    pub fn get_route(&self, index: usize) -> Option<&Arc<Route>> {
        self.routes.get(index)
    }

    /// Get all routes
    #[inline]
    pub fn routes(&self) -> &[Arc<Route>] {
        &self.routes
    }

    /// Check if the collection is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Clear all routes from the collection
    pub fn clear(&mut self) {
        self.routes.clear();
        self.quadtree = Quadtree::new(self.config.reference_pixel_viewport, self.config.bias);
        self.cached_stats = CachedStats::default();
    }

    /// Get the combined bounding box of all routes in WGS84 coordinates (lat/lon)
    ///
    /// This is O(1) as the bounding box is cached and updated incrementally.
    /// Returns `None` if there are no routes loaded.
    /// Returns `Some((min_lat, min_lon, max_lat, max_lon))` otherwise.
    pub fn bounding_box_wgs84(&self) -> Option<(f64, f64, f64, f64)> {
        let bbox = self.cached_stats.bounding_box_mercator?;

        // Convert from Web Mercator back to WGS84
        let (min_lat, min_lon) = utils::mercator_to_wgs84(bbox.min().x, bbox.min().y);
        let (max_lat, max_lon) = utils::mercator_to_wgs84(bbox.max().x, bbox.max().y);

        Some((min_lat, min_lon, max_lat, max_lon))
    }

    /// Get the center point of all routes in WGS84 coordinates
    ///
    /// Returns `None` if there are no routes loaded.
    /// Returns `Some((lat, lon))` otherwise.
    #[inline]
    pub fn center_wgs84(&self) -> Option<(f64, f64)> {
        self.bounding_box_wgs84()
            .map(|(min_lat, min_lon, max_lat, max_lon)| {
                ((min_lat + max_lat) / 2.0, (min_lon + max_lon) / 2.0)
            })
    }

    /// Update cached statistics when a route is added
    #[inline]
    fn update_stats_for_added_route(&mut self, route: &Route) {
        // Update point count
        self.cached_stats.total_points += route.total_points();

        // Update total distance
        self.cached_stats.total_distance += route.total_distance();

        // Update bounding box
        let route_bbox = route.bounding_box();
        match &mut self.cached_stats.bounding_box_mercator {
            Some(bbox) => {
                // Expand existing bounding box
                let new_min_x = bbox.min().x.min(route_bbox.min().x);
                let new_min_y = bbox.min().y.min(route_bbox.min().y);
                let new_max_x = bbox.max().x.max(route_bbox.max().x);
                let new_max_y = bbox.max().y.max(route_bbox.max().y);
                *bbox = Rect::new(
                    geo::Coord {
                        x: new_min_x,
                        y: new_min_y,
                    },
                    geo::Coord {
                        x: new_max_x,
                        y: new_max_y,
                    },
                );
            }
            None => {
                // First route, just use its bounding box
                self.cached_stats.bounding_box_mercator = Some(route_bbox);
            }
        }
    }

    /// Rebuild cached statistics from scratch
    ///
    /// This is useful after deserialization or if the cache becomes invalid.
    #[allow(dead_code)]
    fn rebuild_cached_stats(&mut self) {
        self.cached_stats = CachedStats::default();

        for route in &self.routes {
            self.cached_stats.total_points += route.total_points();
            self.cached_stats.total_distance += route.total_distance();

            let route_bbox = route.bounding_box();
            match &mut self.cached_stats.bounding_box_mercator {
                Some(bbox) => {
                    let new_min_x = bbox.min().x.min(route_bbox.min().x);
                    let new_min_y = bbox.min().y.min(route_bbox.min().y);
                    let new_max_x = bbox.max().x.max(route_bbox.max().x);
                    let new_max_y = bbox.max().y.max(route_bbox.max().y);
                    *bbox = Rect::new(
                        geo::Coord {
                            x: new_min_x,
                            y: new_min_y,
                        },
                        geo::Coord {
                            x: new_max_x,
                            y: new_max_y,
                        },
                    );
                }
                None => {
                    self.cached_stats.bounding_box_mercator = Some(route_bbox);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpx::{Gpx, Track, TrackSegment, Waypoint};

    fn create_test_waypoint(lat: f64, lon: f64) -> Waypoint {
        Waypoint::new(geo::Point::new(lon, lat))
    }

    fn create_test_gpx() -> Gpx {
        let mut gpx = Gpx::default();
        let mut track = Track::default();
        let mut segment = TrackSegment::default();

        // Add test points (around London)
        for i in 0..100 {
            segment.points.push(create_test_waypoint(
                51.5074 + i as f64 * 0.001,
                -0.1278 + i as f64 * 0.001,
            ));
        }

        track.segments.push(segment);
        gpx.tracks.push(track);
        gpx
    }

    #[test]
    fn test_collection_creation() {
        let config = Config::default();
        let collection = RouteCollection::new(config);
        assert_eq!(collection.route_count(), 0);
        assert!(collection.is_empty());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.bias, 1.0);
        assert_eq!(config.max_points_per_node, 100);
    }

    #[test]
    fn test_add_route() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        let result = collection.add_route(gpx);
        assert!(result.is_ok());
        assert_eq!(collection.route_count(), 1);
        assert_eq!(collection.total_points(), 100);
        assert!(!collection.is_empty());
    }

    #[test]
    fn test_query_visible() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Add a route
        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        // Query with a viewport that should contain the route
        use crate::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let screen_size = (1920.0, 1080.0);
        let segments = collection.query_visible(viewport, screen_size);
        // Should return at least one segment
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_add_multiple_routes() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Add multiple routes
        for _ in 0..5 {
            let gpx = create_test_gpx();
            collection.add_route(gpx).unwrap();
        }

        assert_eq!(collection.route_count(), 5);
        assert_eq!(collection.total_points(), 500);
    }

    #[test]
    fn test_add_routes_parallel() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Create multiple GPX data
        let gpx_vec: Vec<Gpx> = (0..10).map(|_| create_test_gpx()).collect();

        // Add in parallel
        let result = collection.add_routes_parallel(gpx_vec);
        assert!(result.is_ok());
        assert_eq!(collection.route_count(), 10);
        assert_eq!(collection.total_points(), 1000);
    }

    #[test]
    fn test_query_empty_viewport() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Add a route around London (51.5N, -0.1W)
        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        // Query with a viewport far away from the route (Japan area)
        use crate::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(35.0, 135.0);
        let max = wgs84_to_mercator(36.0, 136.0);
        let viewport = Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let screen_size = (1920.0, 1080.0);
        let segments = collection.query_visible(viewport, screen_size);
        // Should return no segments for this viewport
        assert!(segments.is_empty());
    }

    #[test]
    fn test_large_route() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Create a large route with many points
        let mut gpx = Gpx::default();
        let mut track = Track::default();
        let mut segment = TrackSegment::default();

        // Add 10,000 points
        for i in 0..10000 {
            segment.points.push(create_test_waypoint(
                51.5074 + (i as f64 * 0.00001),
                -0.1278 + (i as f64 * 0.00001),
            ));
        }

        track.segments.push(segment);
        gpx.tracks.push(track);

        let result = collection.add_route(gpx);
        assert!(result.is_ok());
        assert_eq!(collection.total_points(), 10000);

        // Query should still work efficiently
        use crate::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let screen_size = (1920.0, 1080.0);
        let segments = collection.query_visible(viewport, screen_size);
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_get_info() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        let info = collection.get_info();
        assert_eq!(info.route_count, 1);
        assert_eq!(info.total_points, 100);
        assert!(info.total_distance_meters > 0.0);
    }

    #[test]
    fn test_get_route() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        assert!(collection.get_route(0).is_some());
        assert!(collection.get_route(1).is_none());
    }

    #[test]
    fn test_clear() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();
        assert_eq!(collection.route_count(), 1);

        collection.clear();
        assert_eq!(collection.route_count(), 0);
        assert!(collection.is_empty());
        assert_eq!(collection.total_points(), 0);
        assert_eq!(collection.total_distance(), 0.0);
    }

    #[test]
    fn test_collection_info_default() {
        let info = CollectionInfo::default();
        assert_eq!(info.route_count, 0);
        assert_eq!(info.total_points, 0);
        assert_eq!(info.total_distance_meters, 0.0);
    }

    #[test]
    fn test_bounding_box_wgs84_empty() {
        let config = Config::default();
        let collection = RouteCollection::new(config);
        assert!(collection.bounding_box_wgs84().is_none());
    }

    #[test]
    fn test_bounding_box_wgs84_with_route() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        let bbox = collection.bounding_box_wgs84();
        assert!(bbox.is_some());

        let (min_lat, min_lon, max_lat, max_lon) = bbox.unwrap();
        // Test route is around London (51.5N, -0.1W)
        assert!(min_lat > 51.0 && min_lat < 52.0);
        assert!(max_lat > 51.0 && max_lat < 52.0);
        assert!(min_lon > -1.0 && min_lon < 1.0);
        assert!(max_lon > -1.0 && max_lon < 1.0);
        assert!(min_lat <= max_lat);
        assert!(min_lon <= max_lon);
    }

    #[test]
    fn test_center_wgs84() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        let center = collection.center_wgs84();
        assert!(center.is_some());

        let (lat, lon) = center.unwrap();
        // Center should be around London
        assert!(lat > 51.0 && lat < 52.0);
        assert!(lon > -1.0 && lon < 1.0);
    }

    #[test]
    fn test_query_visible_different_screen_sizes() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        // Query with a viewport that should contain the route
        use crate::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        // Test with different screen sizes
        let small_screen = (800.0, 600.0);
        let large_screen = (3840.0, 2160.0); // 4K

        let results_small = collection.query_visible(viewport, small_screen);
        let results_large = collection.query_visible(viewport, large_screen);

        // Both should return results
        assert!(!results_small.is_empty());
        assert!(!results_large.is_empty());
    }

    #[test]
    fn test_query_with_many_points_segment() {
        // Test that segments with exactly 64 points don't cause overflow
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Create a route with exactly 64 points
        let mut gpx = Gpx::default();
        let mut track = Track::default();
        let mut segment = TrackSegment::default();

        for i in 0..64 {
            segment.points.push(create_test_waypoint(
                51.5074 + i as f64 * 0.0001,
                -0.1278 + i as f64 * 0.0001,
            ));
        }

        track.segments.push(segment);
        gpx.tracks.push(track);

        collection.add_route(gpx).unwrap();

        // Query should not panic
        use crate::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let screen_size = (1920.0, 1080.0);
        let results = collection.query_visible(viewport, screen_size);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_query_with_more_than_64_points_segment() {
        // Test that segments with more than 64 points work correctly
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Create a route with 100 points
        let mut gpx = Gpx::default();
        let mut track = Track::default();
        let mut segment = TrackSegment::default();

        for i in 0..100 {
            segment.points.push(create_test_waypoint(
                51.5074 + i as f64 * 0.0001,
                -0.1278 + i as f64 * 0.0001,
            ));
        }

        track.segments.push(segment);
        gpx.tracks.push(track);

        collection.add_route(gpx).unwrap();

        // Query should not panic
        use crate::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let screen_size = (1920.0, 1080.0);
        let results = collection.query_visible(viewport, screen_size);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_cached_stats_consistency() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Add multiple routes
        for _ in 0..5 {
            let gpx = create_test_gpx();
            collection.add_route(gpx).unwrap();
        }

        // Verify cached stats match expected values
        assert_eq!(collection.total_points(), 500);

        // Clear and verify stats reset
        collection.clear();
        assert_eq!(collection.total_points(), 0);
        assert_eq!(collection.total_distance(), 0.0);
        assert!(collection.bounding_box_wgs84().is_none());
    }

    #[test]
    fn test_incremental_bounding_box() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Add first route
        let gpx1 = create_test_gpx();
        collection.add_route(gpx1).unwrap();

        let bbox1 = collection.bounding_box_wgs84().unwrap();

        // Add second route in a different area
        let mut gpx2 = Gpx::default();
        let mut track = Track::default();
        let mut segment = TrackSegment::default();
        for i in 0..100 {
            segment.points.push(create_test_waypoint(
                52.5 + i as f64 * 0.001, // Different latitude
                0.1 + i as f64 * 0.001,  // Different longitude
            ));
        }
        track.segments.push(segment);
        gpx2.tracks.push(track);
        collection.add_route(gpx2).unwrap();

        let bbox2 = collection.bounding_box_wgs84().unwrap();

        // Combined bounding box should be larger
        assert!(
            bbox2.0 <= bbox1.0 || bbox2.1 <= bbox1.1 || bbox2.2 >= bbox1.2 || bbox2.3 >= bbox1.3
        );
    }
}

//! RouteCollection - Top-level manager for routes, quadtree, and queries

use crate::data::{Quadtree, Result, Route};
use geo::Rect;
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;

/// Configuration for the route collection
#[derive(Debug, Clone)]
pub struct Config {
    /// Reference pixel viewport used for LOD calculations
    pub reference_pixel_viewport: geo::Rect<f64>,
    /// LOD bias factor (default 1.0 = 1 pixel minimum)
    pub bias: f64,
    /// Subdivision threshold for quadtree nodes
    pub max_points_per_node: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            reference_pixel_viewport: geo::Rect::new(
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
#[derive(Debug, Clone)]
pub struct CollectionInfo {
    /// Number of routes loaded
    pub route_count: usize,
    /// Total number of track points
    pub total_points: usize,
    /// Total distance in meters
    pub total_distance_meters: f64,
}

/// Top-level manager for all routes and queries
pub struct RouteCollection {
    routes: Vec<Arc<Route>>,
    quadtree: Quadtree,
    config: Config,
}

impl RouteCollection {
    /// Create a new route collection with the given configuration
    pub fn new(config: Config) -> Self {
        let quadtree = Quadtree::new(config.reference_pixel_viewport, config.bias);
        Self {
            routes: Vec::new(),
            quadtree,
            config,
        }
    }

    /// Add a route to the collection
    pub fn add_route(&mut self, gpx_data: gpx::Gpx) -> Result<()> {
        let route = Route::new(gpx_data)?;

        // Build quadtree for this route
        let route_quadtree = Quadtree::new_with_route(
            route.clone(),
            self.config.reference_pixel_viewport,
            self.config.bias,
        )?;

        // Merge into main quadtree
        self.quadtree.merge(route_quadtree)?;

        // Store route
        self.routes.push(route);

        Ok(())
    }

    /// Add multiple routes in parallel
    pub fn add_routes_parallel(&mut self, gpx_data_vec: Vec<gpx::Gpx>) -> Result<()> {
        // Parse and build quadtrees in parallel
        let results: Result<Vec<(Arc<Route>, Quadtree)>> = gpx_data_vec
            .into_par_iter()
            .map(|gpx_data| {
                let route = Route::new(gpx_data)?;
                let quadtree = Quadtree::new_with_route(
                    route.clone(),
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
            self.routes.push(route);
        }

        Ok(())
    }

    /// Load routes from GPX files in parallel
    pub fn load_from_files<P: AsRef<Path> + Send>(&mut self, paths: Vec<P>) -> Result<()>
    where
        P: Sync,
    {
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
    pub fn query_visible(&self, geo_viewport: Rect<f64>) -> Vec<&crate::data::SimplifiedSegment> {
        self.quadtree.query(geo_viewport)
    }

    /// Get total number of routes
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Get total number of points across all routes
    pub fn total_points(&self) -> usize {
        self.routes.iter().map(|r| r.total_points()).sum()
    }

    /// Get total distance across all routes in meters
    pub fn total_distance(&self) -> f64 {
        self.routes.iter().map(|r| r.total_distance()).sum()
    }

    /// Get collection information
    pub fn get_info(&self) -> CollectionInfo {
        CollectionInfo {
            route_count: self.route_count(),
            total_points: self.total_points(),
            total_distance_meters: self.total_distance(),
        }
    }

    /// Get a reference to the configuration
    pub fn config(&self) -> &Config {
        &self.config
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
    }

    #[test]
    fn test_query_visible() {
        let config = Config::default();
        let mut collection = RouteCollection::new(config);

        // Add a route
        let gpx = create_test_gpx();
        collection.add_route(gpx).unwrap();

        // Query with a viewport that should contain the route
        use crate::data::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = geo::Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let segments = collection.query_visible(viewport);
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

        // Query with a viewport far away from the route
        // Using coordinates in Asia far from London
        use crate::data::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(35.0, 135.0); // Japan area
        let max = wgs84_to_mercator(36.0, 136.0);
        let viewport = geo::Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let segments = collection.query_visible(viewport);
        // Should return no segments for this viewport which is far from the route
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
        use crate::data::utils::wgs84_to_mercator;
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(51.6, -0.0);
        let viewport = geo::Rect::new(
            geo::Coord {
                x: min.x(),
                y: min.y(),
            },
            geo::Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let segments = collection.query_visible(viewport);
        assert!(!segments.is_empty());
    }
}

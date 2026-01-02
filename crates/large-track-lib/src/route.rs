//! Route storage and parsing module
//!
//! This module provides the `Route` struct for storing parsed GPX data
//! with precomputed metadata like bounding boxes and distances.

use crate::{DataError, Result, utils};
use geo::Rect;
use std::sync::Arc;

/// Represents a single GPX route with raw data and precomputed metadata
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Route {
    /// The original GPX data
    gpx_data: gpx::Gpx,
    /// Precomputed bounding box in Web Mercator meters
    bounding_box_mercator: Rect<f64>,
    /// Cached total number of points (computed once during construction)
    cached_total_points: usize,
    /// Cached total distance in meters (computed once during construction)
    cached_total_distance: f64,
}

#[cfg_attr(feature = "profiling", profiling::all_functions)]
impl Route {
    /// Create a new Route from GPX data
    ///
    /// # Arguments
    /// * `gpx_data` - Parsed GPX data containing tracks
    ///
    /// # Returns
    /// An `Arc<Route>` on success, or an error if the route is empty or invalid
    pub fn new(gpx_data: gpx::Gpx) -> Result<Arc<Self>> {
        // High-level profiling scope for route construction.
        // This helps attribute time spent parsing and building route metadata.
        #[cfg(feature = "profiling")]
        profiling::scope!("route::new");
        // Compute all metadata in a single pass
        let (bounding_box_mercator, total_points, total_distance) =
            Self::compute_metadata(&gpx_data)?;

        if total_points == 0 {
            return Err(DataError::EmptyRoute);
        }

        Ok(Arc::new(Route {
            gpx_data,
            bounding_box_mercator,
            cached_total_points: total_points,
            cached_total_distance: total_distance,
        }))
    }

    /// Compute all metadata in a single pass over the data
    ///
    /// Returns (bounding_box, total_points, total_distance)
    fn compute_metadata(gpx: &gpx::Gpx) -> Result<(Rect<f64>, usize, f64)> {
        // Profiling scope for metadata computation (bounding box, counts, distance).
        // This is useful to separate parsing time from metadata computation in traces.
        #[cfg(feature = "profiling")]
        profiling::scope!("route::compute_metadata");
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        let mut total_points: usize = 0;
        let mut total_distance: f64 = 0.0;
        let mut found_valid_point = false;

        for track in &gpx.tracks {
            for segment in &track.segments {
                let points = &segment.points;
                let segment_len = points.len();
                total_points += segment_len;

                let mut prev_waypoint: Option<&gpx::Waypoint> = None;

                for waypoint in points {
                    let point = utils::waypoint_to_mercator(waypoint);

                    if !utils::is_valid_mercator(&point) {
                        tracing::warn!(
                            "Skipping point outside Web Mercator bounds: ({}, {})",
                            waypoint.point().y(),
                            waypoint.point().x()
                        );
                        prev_waypoint = None; // Break distance chain
                        continue;
                    }

                    // Update bounding box
                    min_x = min_x.min(point.x());
                    min_y = min_y.min(point.y());
                    max_x = max_x.max(point.x());
                    max_y = max_y.max(point.y());
                    found_valid_point = true;

                    // Compute distance from previous point
                    if let Some(prev) = prev_waypoint {
                        total_distance += Self::haversine_distance(prev, waypoint);
                    }
                    prev_waypoint = Some(waypoint);
                }
            }
        }

        if !found_valid_point {
            return Err(DataError::InvalidGeometry(
                "No valid points in route".to_string(),
            ));
        }

        let bounding_box = Rect::new(
            geo::Coord { x: min_x, y: min_y },
            geo::Coord { x: max_x, y: max_y },
        );

        Ok((bounding_box, total_points, total_distance))
    }

    /// Get the bounding box in Web Mercator meters
    #[inline]
    pub fn bounding_box(&self) -> Rect<f64> {
        self.bounding_box_mercator
    }

    /// Access the raw GPX data
    #[inline]
    pub fn gpx_data(&self) -> &gpx::Gpx {
        &self.gpx_data
    }

    /// Get all tracks
    #[inline]
    pub fn tracks(&self) -> &[gpx::Track] {
        &self.gpx_data.tracks
    }

    /// Get a specific waypoint by track, segment, and point indices
    #[inline]
    pub fn get_waypoint(
        &self,
        track_index: usize,
        segment_index: usize,
        point_index: usize,
    ) -> Option<&gpx::Waypoint> {
        self.gpx_data
            .tracks
            .get(track_index)?
            .segments
            .get(segment_index)?
            .points
            .get(point_index)
    }

    /// Get total number of points across all tracks and segments
    ///
    /// This is O(1) as the value is cached during construction.
    #[inline]
    pub fn total_points(&self) -> usize {
        self.cached_total_points
    }

    /// Calculate total distance across all tracks and segments in meters
    ///
    /// This is O(1) as the value is cached during construction.
    /// Uses the Haversine formula for accurate distance calculation on a sphere.
    #[inline]
    pub fn total_distance(&self) -> f64 {
        self.cached_total_distance
    }

    /// Calculate the Haversine distance between two waypoints in meters
    #[inline]
    fn haversine_distance(p1: &gpx::Waypoint, p2: &gpx::Waypoint) -> f64 {
        let point1 = p1.point();
        let point2 = p2.point();

        let lat1 = point1.y().to_radians();
        let lat2 = point2.y().to_radians();
        let delta_lat = (point2.y() - point1.y()).to_radians();
        let delta_lon = (point2.x() - point1.x()).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

        // Earth's radius in meters
        const EARTH_RADIUS_M: f64 = 6371000.0;
        EARTH_RADIUS_M * c
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

        // Add a few test points (around London)
        segment.points.push(create_test_waypoint(51.5074, -0.1278));
        segment.points.push(create_test_waypoint(51.5076, -0.1276));
        segment.points.push(create_test_waypoint(51.5078, -0.1274));

        track.segments.push(segment);
        gpx.tracks.push(track);
        gpx
    }

    #[test]
    fn test_route_creation() {
        let gpx = create_test_gpx();
        let route = Route::new(gpx).unwrap();

        assert_eq!(route.total_points(), 3);
        assert_eq!(route.tracks().len(), 1);
    }

    #[test]
    fn test_empty_route_fails() {
        let gpx = Gpx::default();
        let result = Route::new(gpx);
        assert!(result.is_err());
    }

    #[test]
    fn test_bounding_box() {
        let gpx = create_test_gpx();
        let route = Route::new(gpx).unwrap();

        let bbox = route.bounding_box();
        assert!(bbox.width() > 0.0);
        assert!(bbox.height() > 0.0);
    }

    #[test]
    fn test_get_waypoint() {
        let gpx = create_test_gpx();
        let route = Route::new(gpx).unwrap();

        let waypoint = route.get_waypoint(0, 0, 0);
        assert!(waypoint.is_some());

        let waypoint = route.get_waypoint(0, 0, 100);
        assert!(waypoint.is_none());
    }

    #[test]
    fn test_total_distance() {
        let gpx = create_test_gpx();
        let route = Route::new(gpx).unwrap();

        let distance = route.total_distance();
        // The test points are very close together (around London)
        // Distance should be roughly a few tens of meters
        assert!(distance > 0.0);
        assert!(distance < 1000.0); // Less than 1km
    }

    #[test]
    fn test_cached_values_are_consistent() {
        let gpx = create_test_gpx();
        let route = Route::new(gpx).unwrap();

        // Call multiple times to ensure cached values are returned
        let points1 = route.total_points();
        let points2 = route.total_points();
        assert_eq!(points1, points2);

        let dist1 = route.total_distance();
        let dist2 = route.total_distance();
        assert!((dist1 - dist2).abs() < f64::EPSILON);
    }
}

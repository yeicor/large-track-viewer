//! Route storage and parsing module

use crate::data::{DataError, Result, utils};
use geo::Rect;
use std::sync::Arc;

/// Represents a single GPX route with raw data
#[derive(Clone, Debug)]
pub struct Route {
    /// The original GPX data
    gpx_data: gpx::Gpx,
    /// Precomputed bounding box in Web Mercator meters
    bounding_box_mercator: Rect<f64>,
}

impl Route {
    /// Create a new Route from GPX data
    pub fn new(gpx_data: gpx::Gpx) -> Result<Arc<Self>> {
        // Validate that the route has at least one point
        let has_points = gpx_data.tracks.iter().any(|track| {
            track
                .segments
                .iter()
                .any(|segment| !segment.points.is_empty())
        });

        if !has_points {
            return Err(DataError::EmptyRoute);
        }

        // Compute bounding box from all points
        let bounding_box_mercator = Self::compute_bounding_box(&gpx_data)?;

        Ok(Arc::new(Route {
            gpx_data,
            bounding_box_mercator,
        }))
    }

    /// Compute the bounding box for all points in the GPX data
    fn compute_bounding_box(gpx: &gpx::Gpx) -> Result<Rect<f64>> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        let mut found_point = false;

        for track in &gpx.tracks {
            for segment in &track.segments {
                for waypoint in &segment.points {
                    let point = utils::waypoint_to_mercator(waypoint);

                    if !utils::is_valid_mercator(&point) {
                        tracing::warn!(
                            "Skipping point outside Web Mercator bounds: ({}, {})",
                            waypoint.point().y(),
                            waypoint.point().x()
                        );
                        continue;
                    }

                    min_x = min_x.min(point.x());
                    min_y = min_y.min(point.y());
                    max_x = max_x.max(point.x());
                    max_y = max_y.max(point.y());
                    found_point = true;
                }
            }
        }

        if !found_point {
            return Err(DataError::InvalidGeometry(
                "No valid points in route".to_string(),
            ));
        }

        Ok(Rect::new(
            geo::Coord { x: min_x, y: min_y },
            geo::Coord { x: max_x, y: max_y },
        ))
    }

    /// Get the bounding box in Web Mercator meters
    pub fn bounding_box(&self) -> Rect<f64> {
        self.bounding_box_mercator
    }

    /// Access the raw GPX data
    pub fn gpx_data(&self) -> &gpx::Gpx {
        &self.gpx_data
    }

    /// Get all tracks
    pub fn tracks(&self) -> &[gpx::Track] {
        &self.gpx_data.tracks
    }

    /// Get a specific waypoint by track, segment, and point indices
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
    pub fn total_points(&self) -> usize {
        self.gpx_data
            .tracks
            .iter()
            .map(|track| {
                track
                    .segments
                    .iter()
                    .map(|segment| segment.points.len())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Calculate total distance across all tracks and segments in meters
    /// Uses Haversine formula for accurate distance calculation
    pub fn total_distance(&self) -> f64 {
        let mut total = 0.0;

        for track in &self.gpx_data.tracks {
            for segment in &track.segments {
                let points = &segment.points;
                for i in 0..points.len().saturating_sub(1) {
                    let p1 = points[i].point();
                    let p2 = points[i + 1].point();

                    // Haversine formula for distance between two lat/lon points
                    let lat1 = p1.y().to_radians();
                    let lat2 = p2.y().to_radians();
                    let delta_lat = (p2.y() - p1.y()).to_radians();
                    let delta_lon = (p2.x() - p1.x()).to_radians();

                    let a = (delta_lat / 2.0).sin().powi(2)
                        + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
                    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

                    // Earth's radius in meters
                    const EARTH_RADIUS_M: f64 = 6371000.0;
                    total += EARTH_RADIUS_M * c;
                }
            }
        }

        total
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
        // Distance should be roughly a few hundred meters
        assert!(distance > 0.0);
        assert!(distance < 1000.0); // Less than 1km
    }
}

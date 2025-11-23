//! Segment structures for external indexing into route data

use crate::Route;
use std::ops::Range;
use std::sync::Arc;

/// Represents a simplified line segment at a specific LOD, referencing raw data
#[derive(Clone, Debug)]
pub struct SimplifiedSegment {
    /// Reference to the owning route
    pub route: Arc<Route>,
    /// Multiple connected sub-segments (for routes crossing node boundaries)
    pub parts: Vec<SegmentPart>,
}

/// A single part of a simplified segment
#[derive(Clone, Debug)]
pub struct SegmentPart {
    /// Index of the track in the route
    pub track_index: usize,
    /// Index of the segment in the track
    pub segment_index: usize,
    /// Range of points in the original segment
    pub point_range: Range<usize>,
    /// Indices into point_range for simplified points (relative to point_range.start)
    pub simplified_indices: Vec<usize>,
}

/// Boundary context for rendering continuous lines
#[derive(Debug)]
pub struct BoundaryContext<'a> {
    /// Point immediately before this part (if exists)
    pub prev_point: Option<&'a gpx::Waypoint>,
    /// Point immediately after this part (if exists)
    pub next_point: Option<&'a gpx::Waypoint>,
}

impl SimplifiedSegment {
    /// Create a new simplified segment
    pub fn new(route: Arc<Route>, parts: Vec<SegmentPart>) -> Self {
        Self { route, parts }
    }

    /// Create a simplified segment with a single part
    pub fn single(
        route: Arc<Route>,
        track_index: usize,
        segment_index: usize,
        point_range: Range<usize>,
        simplified_indices: Vec<usize>,
    ) -> Self {
        Self {
            route,
            parts: vec![SegmentPart {
                track_index,
                segment_index,
                point_range,
                simplified_indices,
            }],
        }
    }
}

impl SegmentPart {
    /// Create a new segment part
    pub fn new(
        track_index: usize,
        segment_index: usize,
        point_range: Range<usize>,
        simplified_indices: Vec<usize>,
    ) -> Self {
        Self {
            track_index,
            segment_index,
            point_range,
            simplified_indices,
        }
    }

    /// Get the point immediately before this part (if exists)
    pub fn get_prev_point<'a>(&self, route: &'a Route) -> Option<&'a gpx::Waypoint> {
        if self.point_range.start == 0 {
            return None; // First point in segment
        }

        route
            .gpx_data()
            .tracks
            .get(self.track_index)?
            .segments
            .get(self.segment_index)?
            .points
            .get(self.point_range.start - 1)
    }

    /// Get the point immediately after this part (if exists)
    pub fn get_next_point<'a>(&self, route: &'a Route) -> Option<&'a gpx::Waypoint> {
        let segment = route
            .gpx_data()
            .tracks
            .get(self.track_index)?
            .segments
            .get(self.segment_index)?;

        if self.point_range.end >= segment.points.len() {
            return None; // Last point in segment
        }

        segment.points.get(self.point_range.end)
    }

    /// Get all simplified points for rendering
    pub fn get_simplified_points<'a>(&self, route: &'a Route) -> Vec<&'a gpx::Waypoint> {
        let segment = match route
            .gpx_data()
            .tracks
            .get(self.track_index)
            .and_then(|t| t.segments.get(self.segment_index))
        {
            Some(seg) => seg,
            None => return Vec::new(),
        };

        self.simplified_indices
            .iter()
            .filter_map(|&idx| segment.points.get(self.point_range.start + idx))
            .collect()
    }

    /// Get boundary context for continuous rendering
    pub fn get_boundary_context<'a>(&self, route: &'a Route) -> BoundaryContext<'a> {
        BoundaryContext {
            prev_point: self.get_prev_point(route),
            next_point: self.get_next_point(route),
        }
    }

    /// Get all points including boundary context for rendering
    pub fn get_points_with_context<'a>(&self, route: &'a Route) -> Vec<&'a gpx::Waypoint> {
        let mut points = Vec::new();

        if let Some(prev) = self.get_prev_point(route) {
            points.push(prev);
        }

        points.extend(self.get_simplified_points(route));

        if let Some(next) = self.get_next_point(route) {
            points.push(next);
        }

        points
    }

    /// Check if this part contains any points
    pub fn is_empty(&self) -> bool {
        self.simplified_indices.is_empty()
    }

    /// Get the number of simplified points
    pub fn len(&self) -> usize {
        self.simplified_indices.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpx::{Gpx, Track, TrackSegment, Waypoint};

    fn create_test_waypoint(lat: f64, lon: f64) -> Waypoint {
        Waypoint::new(geo::Point::new(lon, lat))
    }

    fn create_test_route() -> Arc<Route> {
        let mut gpx = Gpx::default();
        let mut track = Track::default();
        let mut segment = TrackSegment::default();

        // Add test points
        for i in 0..10 {
            segment.points.push(create_test_waypoint(
                51.5074 + i as f64 * 0.0001,
                -0.1278 + i as f64 * 0.0001,
            ));
        }

        track.segments.push(segment);
        gpx.tracks.push(track);
        Route::new(gpx).unwrap()
    }

    #[test]
    fn test_segment_part_creation() {
        let _route = create_test_route();
        let part = SegmentPart::new(0, 0, 0..5, vec![0, 2, 4]);

        assert_eq!(part.track_index, 0);
        assert_eq!(part.segment_index, 0);
        assert_eq!(part.point_range, 0..5);
        assert_eq!(part.simplified_indices, vec![0, 2, 4]);
    }

    #[test]
    fn test_get_simplified_points() {
        let route = create_test_route();
        let part = SegmentPart::new(0, 0, 0..5, vec![0, 2, 4]);

        let points = part.get_simplified_points(&route);
        assert_eq!(points.len(), 3);
    }

    #[test]
    fn test_get_prev_point() {
        let route = create_test_route();

        // First point has no previous
        let part1 = SegmentPart::new(0, 0, 0..5, vec![0, 2, 4]);
        assert!(part1.get_prev_point(&route).is_none());

        // Non-first point has previous
        let part2 = SegmentPart::new(0, 0, 2..5, vec![0, 2]);
        assert!(part2.get_prev_point(&route).is_some());
    }

    #[test]
    fn test_get_next_point() {
        let route = create_test_route();

        // Last point has no next
        let part1 = SegmentPart::new(0, 0, 5..10, vec![0, 2, 4]);
        assert!(part1.get_next_point(&route).is_none());

        // Non-last point has next
        let part2 = SegmentPart::new(0, 0, 0..5, vec![0, 2, 4]);
        assert!(part2.get_next_point(&route).is_some());
    }

    #[test]
    fn test_get_points_with_context() {
        let route = create_test_route();
        let part = SegmentPart::new(0, 0, 2..5, vec![0, 1, 2]);

        let points = part.get_points_with_context(&route);
        // Should have: prev + 3 simplified + next = 5 points
        assert_eq!(points.len(), 5);
    }

    #[test]
    fn test_simplified_segment_single() {
        let route = create_test_route();
        let segment = SimplifiedSegment::single(route.clone(), 0, 0, 0..5, vec![0, 2, 4]);

        assert_eq!(segment.parts.len(), 1);
        assert_eq!(segment.parts[0].simplified_indices.len(), 3);
    }
}

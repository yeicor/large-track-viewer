//! Segment structures for external indexing into route data
//!
//! This module provides structures for representing simplified line segments
//! that reference the original route data without duplicating points.

use crate::Route;
use std::ops::Range;
use std::sync::Arc;

/// Represents a simplified line segment at a specific LOD level
///
/// This structure references the original route data and stores only indices,
/// avoiding point duplication while enabling efficient LOD-based rendering.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SimplifiedSegment {
    /// Reference to the owning route
    pub route: Arc<Route>,
    /// Index of this route in the collection (for per-route coloring)
    pub route_index: usize,
    /// Multiple connected sub-segments (for routes crossing node boundaries)
    pub parts: Vec<SegmentPart>,
}

/// A single part of a simplified segment
///
/// Stores indices into the original GPX track data, allowing efficient
/// access to both simplified and full-resolution points.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[cfg_attr(feature = "profiling", profiling::all_functions)]
impl SimplifiedSegment {
    /// Create a new simplified segment
    pub fn new(route: Arc<Route>, route_index: usize, parts: Vec<SegmentPart>) -> Self {
        Self {
            route,
            route_index,
            parts,
        }
    }

    /// Create a simplified segment with a single part
    pub fn single(
        route: Arc<Route>,
        route_index: usize,
        track_index: usize,
        segment_index: usize,
        point_range: Range<usize>,
        simplified_indices: Vec<usize>,
    ) -> Self {
        Self {
            route,
            route_index,
            parts: vec![SegmentPart {
                track_index,
                segment_index,
                point_range,
                simplified_indices,
            }],
        }
    }
}

#[cfg_attr(feature = "profiling", profiling::all_functions)]
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
    ///
    /// This is used for boundary context to ensure smooth line rendering
    /// at the edges of viewport-clipped segments.
    pub fn get_prev_point<'a>(&self, route: &'a Route) -> Option<&'a gpx::Waypoint> {
        // Per-function profiling scope to make boundary context lookups visible in traces.
        #[cfg(feature = "profiling")]
        profiling::scope!("segment::get_prev_point");

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
    ///
    /// This is used for boundary context to ensure smooth line rendering
    /// at the edges of viewport-clipped segments.
    pub fn get_next_point<'a>(&self, route: &'a Route) -> Option<&'a gpx::Waypoint> {
        // Profiling scope for next-point lookup to help trace boundary handling.
        #[cfg(feature = "profiling")]
        profiling::scope!("segment::get_next_point");

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
    ///
    /// Returns references to the waypoints at the simplified indices,
    /// providing the LOD-reduced representation of this segment part.
    pub fn get_simplified_points<'a>(&self, route: &'a Route) -> Vec<&'a gpx::Waypoint> {
        // Make simplification point retrieval visible in traces; these calls are often hot.
        #[cfg(feature = "profiling")]
        profiling::scope!("segment::get_simplified_points");

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

    /// Get all points including boundary context for rendering
    ///
    /// This includes the previous point (if any), all simplified points,
    /// and the next point (if any), enabling smooth line rendering at
    /// segment boundaries.
    pub fn get_points_with_context<'a>(&self, route: &'a Route) -> Vec<&'a gpx::Waypoint> {
        // Profiling scope to attribute cost of assembling points with surrounding context.
        #[cfg(feature = "profiling")]
        profiling::scope!("segment::get_points_with_context");

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

    /// Get the full-resolution points in this part's range
    ///
    /// Returns all original waypoints without simplification.
    pub fn get_full_points<'a>(&self, route: &'a Route) -> Vec<&'a gpx::Waypoint> {
        // Profiling scope for full-resolution point extraction; useful when diagnosing
        // where rendering or geometry work originates in traces.
        #[cfg(feature = "profiling")]
        profiling::scope!("segment::get_full_points");

        let segment = match route
            .gpx_data()
            .tracks
            .get(self.track_index)
            .and_then(|t| t.segments.get(self.segment_index))
        {
            Some(seg) => seg,
            None => return Vec::new(),
        };

        (self.point_range.start..self.point_range.end)
            .filter_map(|idx| segment.points.get(idx))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_route() -> Arc<Route> {
        let mut gpx = gpx::Gpx::default();
        let mut track = gpx::Track::default();
        let mut segment = gpx::TrackSegment::default();

        // Add test points
        for i in 0..10 {
            segment.points.push(gpx::Waypoint::new(geo::Point::new(
                -0.1278 + i as f64 * 0.0001,
                51.5074 + i as f64 * 0.0001,
            )));
        }

        track.segments.push(segment);
        gpx.tracks.push(track);
        Route::new(gpx).unwrap()
    }

    #[test]
    fn test_segment_part_new() {
        let part = SegmentPart::new(0, 1, 10..20, vec![0, 3, 6, 9]);
        assert_eq!(part.track_index, 0);
        assert_eq!(part.segment_index, 1);
        assert_eq!(part.point_range, 10..20);
        assert_eq!(part.simplified_indices, vec![0, 3, 6, 9]);
    }

    #[test]
    fn test_simplified_segment_new() {
        let route = create_test_route();

        let parts = vec![SegmentPart::new(0, 0, 0..10, vec![0, 5, 9])];
        let segment = SimplifiedSegment::new(route.clone(), 0, parts);

        assert_eq!(segment.route_index, 0);
        assert_eq!(segment.parts.len(), 1);
    }

    #[test]
    fn test_simplified_segment_single() {
        let route = create_test_route();

        let segment = SimplifiedSegment::single(route.clone(), 2, 0, 0, 0..10, vec![0, 5, 9]);

        assert_eq!(segment.route_index, 2);
        assert_eq!(segment.parts.len(), 1);
        assert_eq!(segment.parts[0].track_index, 0);
        assert_eq!(segment.parts[0].segment_index, 0);
    }
}

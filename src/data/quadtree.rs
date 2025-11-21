//! Quadtree spatial index for efficient LOD-based querying

use crate::data::{DataError, Result, Route, SegmentPart, SimplifiedSegment, utils};
use geo::{LineString, Point, Rect};
use std::sync::Arc;

/// Root container for the quadtree index
#[derive(Debug)]
pub struct Quadtree {
    root: QuadtreeNode,
    reference_pixel_viewport: Rect<f64>,
    bias: f64,
}

/// A single node in the LOD quadtree
#[derive(Debug)]
struct QuadtreeNode {
    bounding_box: Rect<f64>,
    level: u32,
    pixel_tolerance: f64,
    segments: Vec<SimplifiedSegment>,
    children: Option<Box<[QuadtreeNode; 4]>>,
}

impl Quadtree {
    /// Create a new empty quadtree with Earth bounds
    pub fn new(reference_pixel_viewport: Rect<f64>, bias: f64) -> Self {
        Self {
            root: QuadtreeNode::new_root(reference_pixel_viewport, bias),
            reference_pixel_viewport,
            bias,
        }
    }

    /// Build a quadtree for a single route (parallelizable)
    pub fn new_with_route(route: Arc<Route>, pixel_viewport: Rect<f64>, bias: f64) -> Result<Self> {
        let mut quadtree = Self::new(pixel_viewport, bias);

        // Insert all track segments from the route
        for (track_idx, track) in route.tracks().iter().enumerate() {
            for (segment_idx, segment) in track.segments.iter().enumerate() {
                if segment.points.is_empty() {
                    continue;
                }

                // Convert points to Web Mercator
                let mercator_points: Vec<Point<f64>> = segment
                    .points
                    .iter()
                    .map(utils::waypoint_to_mercator)
                    .collect();

                // Insert into quadtree
                quadtree.root.insert_segment(
                    route.clone(),
                    track_idx,
                    segment_idx,
                    0..segment.points.len(),
                    &mercator_points,
                    pixel_viewport,
                    bias,
                    100, // subdivision threshold
                )?;
            }
        }

        Ok(quadtree)
    }

    /// Merge another quadtree into this one
    pub fn merge(&mut self, other: Quadtree) -> Result<()> {
        // Verify compatibility
        if self.reference_pixel_viewport != other.reference_pixel_viewport {
            return Err(DataError::MergeMismatch {
                reason: "Pixel viewports do not match".to_string(),
            });
        }
        if (self.bias - other.bias).abs() > 1e-6 {
            return Err(DataError::MergeMismatch {
                reason: "Bias values do not match".to_string(),
            });
        }

        // Merge root nodes
        self.root.merge_with(other.root)?;
        Ok(())
    }

    /// Query for segments intersecting the viewport
    pub fn query(&self, geo_viewport: Rect<f64>) -> Vec<&SimplifiedSegment> {
        // Calculate target level based on viewport size
        let target_level = self.calculate_target_level(geo_viewport);

        // Query tree at that level
        let mut results = Vec::new();
        self.root
            .query_at_level(geo_viewport, target_level, &mut results);
        results
    }

    /// Calculate the appropriate LOD level for the given viewport
    fn calculate_target_level(&self, geo_viewport: Rect<f64>) -> u32 {
        use crate::data::utils::EARTH_SIZE_METERS;

        let viewport_width_meters = geo_viewport.width();

        // Find level where nodes are ~1-2 node widths visible
        let mut level = 0;
        let mut node_width = EARTH_SIZE_METERS;

        while node_width > viewport_width_meters / 2.0 && level < 30 {
            level += 1;
            node_width /= 2.0;
        }

        level
    }
}

impl QuadtreeNode {
    /// Create a root node covering the entire Earth
    fn new_root(reference_pixel_viewport: Rect<f64>, bias: f64) -> Self {
        use crate::data::utils::{EARTH_MERCATOR_MAX, EARTH_MERCATOR_MIN};

        let bounding_box = Rect::new(
            geo::Coord {
                x: EARTH_MERCATOR_MIN,
                y: EARTH_MERCATOR_MIN,
            },
            geo::Coord {
                x: EARTH_MERCATOR_MAX,
                y: EARTH_MERCATOR_MAX,
            },
        );

        let pixel_tolerance = Self::calculate_pixel_tolerance(0, reference_pixel_viewport, bias);

        Self {
            bounding_box,
            level: 0,
            pixel_tolerance,
            segments: Vec::new(),
            children: None,
        }
    }

    /// Calculate pixel tolerance for a given level
    fn calculate_pixel_tolerance(level: u32, pixel_viewport: Rect<f64>, bias: f64) -> f64 {
        use crate::data::utils::EARTH_SIZE_METERS;

        let node_size_meters = EARTH_SIZE_METERS / (1u64 << level) as f64;
        let pixels_per_meter = pixel_viewport.width() / node_size_meters;
        bias / pixels_per_meter
    }

    /// Subdivide this node into 4 children
    fn subdivide(&mut self, pixel_viewport: Rect<f64>, bias: f64) {
        if self.children.is_some() {
            return; // Already subdivided
        }

        let min = self.bounding_box.min();
        let max = self.bounding_box.max();
        let mid_x = (min.x + max.x) / 2.0;
        let mid_y = (min.y + max.y) / 2.0;

        let child_level = self.level + 1;
        let child_tolerance = Self::calculate_pixel_tolerance(child_level, pixel_viewport, bias);

        // Create 4 children: NW, NE, SW, SE
        let nw = QuadtreeNode {
            bounding_box: Rect::new(
                geo::Coord { x: min.x, y: mid_y },
                geo::Coord { x: mid_x, y: max.y },
            ),
            level: child_level,
            pixel_tolerance: child_tolerance,
            segments: Vec::new(),
            children: None,
        };

        let ne = QuadtreeNode {
            bounding_box: Rect::new(
                geo::Coord { x: mid_x, y: mid_y },
                geo::Coord { x: max.x, y: max.y },
            ),
            level: child_level,
            pixel_tolerance: child_tolerance,
            segments: Vec::new(),
            children: None,
        };

        let sw = QuadtreeNode {
            bounding_box: Rect::new(
                geo::Coord { x: min.x, y: min.y },
                geo::Coord { x: mid_x, y: mid_y },
            ),
            level: child_level,
            pixel_tolerance: child_tolerance,
            segments: Vec::new(),
            children: None,
        };

        let se = QuadtreeNode {
            bounding_box: Rect::new(
                geo::Coord { x: mid_x, y: min.y },
                geo::Coord { x: max.x, y: mid_y },
            ),
            level: child_level,
            pixel_tolerance: child_tolerance,
            segments: Vec::new(),
            children: None,
        };

        self.children = Some(Box::new([nw, ne, sw, se]));
    }

    /// Insert a segment into this node or its children
    fn insert_segment(
        &mut self,
        route: Arc<Route>,
        track_idx: usize,
        segment_idx: usize,
        point_range: std::ops::Range<usize>,
        mercator_points: &[Point<f64>],
        pixel_viewport: Rect<f64>,
        bias: f64,
        subdivision_threshold: usize,
    ) -> Result<()> {
        // Clip segment to this node's bounds
        let clipped_ranges =
            clip_linestring_to_rect(&mercator_points[point_range.clone()], self.bounding_box);

        if clipped_ranges.is_empty() {
            return Ok(()); // Segment doesn't intersect this node
        }

        // For each clipped range, create a simplified segment part
        for clipped_range in clipped_ranges {
            // Adjust range to be relative to the full segment
            let adjusted_range =
                (point_range.start + clipped_range.start)..(point_range.start + clipped_range.end);

            // Get the points for this clipped range
            let clipped_points = &mercator_points[adjusted_range.clone()];

            if clipped_points.is_empty() {
                continue;
            }

            // Simplify at this node's tolerance
            let simplified_indices = simplify_vw_indices(clipped_points, self.pixel_tolerance);

            if simplified_indices.is_empty() {
                continue;
            }

            // Create segment part
            let part = SegmentPart::new(
                track_idx,
                segment_idx,
                adjusted_range.clone(),
                simplified_indices,
            );

            // Add to this node
            self.segments
                .push(SimplifiedSegment::new(route.clone(), vec![part]));
        }

        // Check if we need to subdivide
        if self.segments.len() > subdivision_threshold && self.children.is_none() {
            self.subdivide(pixel_viewport, bias);

            // Move segments to children
            let segments_to_redistribute = std::mem::take(&mut self.segments);

            for segment in segments_to_redistribute {
                for part in &segment.parts {
                    // Get mercator points for this part
                    let part_points: Vec<Point<f64>> = part
                        .get_simplified_points(&segment.route)
                        .iter()
                        .map(|wp| utils::waypoint_to_mercator(wp))
                        .collect();

                    if part_points.is_empty() {
                        continue;
                    }

                    // Try to insert into each child
                    if let Some(children) = &mut self.children {
                        for child in children.iter_mut() {
                            child.insert_segment(
                                segment.route.clone(),
                                part.track_index,
                                part.segment_index,
                                part.point_range.clone(),
                                &part_points,
                                pixel_viewport,
                                bias,
                                subdivision_threshold,
                            )?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Merge another node into this one (must have same bounds and level)
    fn merge_with(&mut self, other: QuadtreeNode) -> Result<()> {
        // Verify nodes are compatible
        if self.level != other.level {
            return Err(DataError::MergeMismatch {
                reason: format!(
                    "Node levels do not match: {} vs {}",
                    self.level, other.level
                ),
            });
        }

        let bbox_eq = (self.bounding_box.min().x - other.bounding_box.min().x).abs() < 1.0
            && (self.bounding_box.min().y - other.bounding_box.min().y).abs() < 1.0
            && (self.bounding_box.max().x - other.bounding_box.max().x).abs() < 1.0
            && (self.bounding_box.max().y - other.bounding_box.max().y).abs() < 1.0;

        if !bbox_eq {
            return Err(DataError::MergeMismatch {
                reason: "Node bounding boxes do not match".to_string(),
            });
        }

        // Merge segments
        self.segments.extend(other.segments);

        // Merge children
        match (&mut self.children, other.children) {
            (None, None) => {
                // Both have no children, nothing to do
            }
            (None, Some(other_children)) => {
                // We have no children but other does, take them
                self.children = Some(other_children);
            }
            (Some(_), None) => {
                // We have children but other doesn't, keep ours
            }
            (Some(self_children), Some(other_children)) => {
                // Both have children, merge recursively
                for (self_child, other_child) in
                    self_children.iter_mut().zip(other_children.into_iter())
                {
                    self_child.merge_with(other_child)?;
                }
            }
        }

        Ok(())
    }

    /// Query this node and its children for segments at the target level
    fn query_at_level<'a>(
        &'a self,
        viewport: Rect<f64>,
        target_level: u32,
        results: &mut Vec<&'a SimplifiedSegment>,
    ) {
        // Frustum culling - check if this node intersects the viewport
        if !self.intersects_viewport(viewport) {
            return;
        }

        // At target level, return segments
        if self.level == target_level {
            results.extend(self.segments.iter());
            return;
        }

        // Too shallow, need to go deeper
        if self.level < target_level {
            if let Some(children) = &self.children {
                for child in children.iter() {
                    child.query_at_level(viewport, target_level, results);
                }
            } else {
                // No children available, return segments that intersect the viewport
                for segment in &self.segments {
                    // Check if any part of the segment intersects the viewport
                    let mut intersects = false;
                    for part in &segment.parts {
                        let part_points = part.get_simplified_points(&segment.route);
                        for waypoint in part_points {
                            let point = utils::waypoint_to_mercator(waypoint);
                            if point.x() >= viewport.min().x
                                && point.x() <= viewport.max().x
                                && point.y() >= viewport.min().y
                                && point.y() <= viewport.max().y
                            {
                                intersects = true;
                                break;
                            }
                        }
                        if intersects {
                            break;
                        }
                    }
                    if intersects {
                        results.push(segment);
                    }
                }
            }
        }
        // If too deep (self.level > target_level), return nothing
    }

    /// Check if this node intersects the viewport
    fn intersects_viewport(&self, viewport: Rect<f64>) -> bool {
        let min = self.bounding_box.min();
        let max = self.bounding_box.max();
        let vmin = viewport.min();
        let vmax = viewport.max();

        // Check for intersection
        !(max.x < vmin.x || min.x > vmax.x || max.y < vmin.y || min.y > vmax.y)
    }
}

/// Clip a linestring to a rectangle, returning ranges of points that fall within
fn clip_linestring_to_rect(
    points_mercator: &[Point<f64>],
    rect: Rect<f64>,
) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut current_start: Option<usize> = None;

    for (i, point) in points_mercator.iter().enumerate() {
        let inside = point.x() >= rect.min().x
            && point.x() <= rect.max().x
            && point.y() >= rect.min().y
            && point.y() <= rect.max().y;

        if inside {
            if current_start.is_none() {
                current_start = Some(i);
            }
        } else if let Some(start) = current_start {
            // End of a range
            ranges.push(start..i);
            current_start = None;
        }
    }

    // Close final range if needed
    if let Some(start) = current_start {
        ranges.push(start..points_mercator.len());
    }

    ranges
}

/// Simplify a linestring using Visvalingam-Whyatt, returning indices into the original
fn simplify_vw_indices(points: &[Point<f64>], tolerance: f64) -> Vec<usize> {
    if points.len() <= 2 {
        return (0..points.len()).collect();
    }

    // Convert to geo::LineString for simplification
    let coords: Vec<geo::Coord<f64>> = points
        .iter()
        .map(|p| geo::Coord { x: p.x(), y: p.y() })
        .collect();
    let linestring = LineString::from(coords);

    // Simplify using Visvalingam-Whyatt
    use geo::SimplifyVw;
    let simplified = linestring.simplify_vw(tolerance);

    // Map simplified points back to original indices
    let mut result_indices = Vec::new();
    for simplified_coord in simplified.coords() {
        // Find the closest match in original (should be exact or very close)
        if let Some((idx, _)) = points.iter().enumerate().min_by_key(|(_, p)| {
            let dx = p.x() - simplified_coord.x;
            let dy = p.y() - simplified_coord.y;
            ((dx * dx + dy * dy) * 1e10) as i64
        }) {
            if !result_indices.contains(&idx) {
                result_indices.push(idx);
            }
        }
    }

    // Ensure indices are sorted
    result_indices.sort_unstable();
    result_indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quadtree_creation() {
        let viewport = Rect::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord {
                x: 1024.0,
                y: 768.0,
            },
        );
        let quadtree = Quadtree::new(viewport, 1.0);
        assert_eq!(quadtree.root.level, 0);
    }

    #[test]
    fn test_pixel_tolerance_calculation() {
        let viewport = Rect::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord {
                x: 1024.0,
                y: 768.0,
            },
        );
        let tolerance = QuadtreeNode::calculate_pixel_tolerance(0, viewport, 1.0);
        assert!(tolerance > 0.0);
    }

    #[test]
    fn test_clip_linestring() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(5.0, 5.0),
            Point::new(10.0, 10.0),
            Point::new(15.0, 15.0),
        ];
        let rect = Rect::new(
            geo::Coord { x: 4.0, y: 4.0 },
            geo::Coord { x: 11.0, y: 11.0 },
        );

        let ranges = clip_linestring_to_rect(&points, rect);
        assert!(!ranges.is_empty());
    }

    #[test]
    fn test_simplify_vw_indices() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.1),
            Point::new(2.0, 0.0),
            Point::new(3.0, 0.1),
            Point::new(4.0, 0.0),
        ];

        let indices = simplify_vw_indices(&points, 0.2);
        assert!(indices.len() <= points.len());
        assert!(indices.len() >= 2); // At least start and end
    }

    #[test]
    fn test_node_subdivide() {
        let viewport = Rect::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord {
                x: 1024.0,
                y: 768.0,
            },
        );
        let mut node = QuadtreeNode::new_root(viewport, 1.0);

        assert!(node.children.is_none());
        node.subdivide(viewport, 1.0);
        assert!(node.children.is_some());

        let children = node.children.as_ref().unwrap();
        assert_eq!(children.len(), 4);
        assert_eq!(children[0].level, 1);
    }
}

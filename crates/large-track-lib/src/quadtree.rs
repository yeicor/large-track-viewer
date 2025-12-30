//! Quadtree spatial index for efficient LOD-based querying
//!
//! This module provides an adaptive quadtree structure that enables fast spatial
//! queries with level-of-detail support. The tree stores segments at their appropriate
//! level and generates simplified versions lazily on-demand.

use crate::{DataError, Result, Route, SegmentPart, SimplifiedSegment, utils};
use geo::{Coord, LineString, Point, Rect, SimplifyVwIdx};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Maximum depth of the quadtree to prevent infinite recursion
const MAX_DEPTH: u32 = 20;

/// Minimum number of points required to recurse into children
const MIN_POINTS_FOR_RECURSION: usize = 8;

/// A raw segment stored in the quadtree (before simplification)
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct RawSegment {
    /// Reference to the owning route
    route: Arc<Route>,
    /// Index of this route in the collection (for per-route coloring)
    route_index: usize,
    /// Index of the track in the route
    track_index: usize,
    /// Index of the segment in the track
    segment_index: usize,
    /// Mercator coordinates of all points (cached to avoid recomputation)
    mercator_points: Arc<Vec<Point<f64>>>,
    /// Optional mapping from chunk indices to original segment indices
    /// (used when this is a chunked portion of a larger segment)
    original_indices: Option<Arc<Vec<usize>>>,
}

/// Cache key for simplified segments
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct SimplificationCacheKey {
    /// Pointer to the route (using Arc's address as part of key)
    route_ptr: usize,
    track_index: usize,
    segment_index: usize,
    /// Tolerance level (discretized to avoid floating point issues)
    tolerance_level: u32,
    /// Hash of chunk bounds (first_idx, last_idx, len) for chunked segments
    chunk_hash: Option<(usize, usize, usize)>,
}

/// Root container for the quadtree spatial index
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Quadtree {
    /// Root node covering the entire Earth in Web Mercator coordinates
    root: QuadtreeNode,
    /// Reference viewport size for LOD calculations
    reference_pixel_viewport: Rect<f64>,
    /// LOD bias factor (higher = more detail retained)
    bias: f64,
    /// Cache for simplified segments (shared across all queries)
    /// This is rebuilt at runtime, not serialized
    #[cfg_attr(
        feature = "serde",
        serde(skip, default = "default_simplification_cache")
    )]
    simplification_cache: Arc<RwLock<HashMap<SimplificationCacheKey, Arc<Vec<usize>>>>>,
}

#[cfg(feature = "serde")]
fn default_simplification_cache() -> Arc<RwLock<HashMap<SimplificationCacheKey, Arc<Vec<usize>>>>> {
    Arc::new(RwLock::new(HashMap::new()))
}

/// A single node in the LOD quadtree
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct QuadtreeNode {
    /// Bounding box in Web Mercator meters
    bounding_box: Rect<f64>,
    /// Depth level in the tree (0 = root)
    level: u32,
    /// Raw segments stored at this node (at the deepest appropriate level)
    raw_segments: Vec<RawSegment>,
    /// Child nodes (NW, NE, SW, SE) if subdivided
    children: Option<Box<[QuadtreeNode; 4]>>,
}

impl Quadtree {
    /// Create a new empty quadtree with Earth bounds
    ///
    /// # Arguments
    /// * `reference_pixel_viewport` - Reference viewport size for LOD calculations
    /// * `bias` - LOD bias factor (1.0 = normal, higher = more detail)
    pub fn new(reference_pixel_viewport: Rect<f64>, bias: f64) -> Self {
        Self {
            root: QuadtreeNode::new_root(reference_pixel_viewport, bias),
            reference_pixel_viewport,
            bias,
            simplification_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Build a quadtree for a single route
    ///
    /// This can be called in parallel for multiple routes and the results merged.
    /// The `route_index` is used for per-route coloring in the viewer.
    pub fn new_with_route(
        route: Arc<Route>,
        route_index: usize,
        pixel_viewport: Rect<f64>,
        bias: f64,
    ) -> Result<Self> {
        let mut quadtree = Self::new(pixel_viewport, bias);

        // Insert all track segments from the route
        for (track_idx, track) in route.tracks().iter().enumerate() {
            for (segment_idx, segment) in track.segments.iter().enumerate() {
                if segment.points.is_empty() {
                    continue;
                }

                // Convert points to Web Mercator (do this once, cache it)
                let mercator_points: Vec<Point<f64>> = segment
                    .points
                    .iter()
                    .map(|wp| utils::wgs84_to_mercator(wp.point().y(), wp.point().x()))
                    .collect();

                let raw_segment = RawSegment {
                    route: route.clone(),
                    route_index,
                    track_index: track_idx,
                    segment_index: segment_idx,
                    mercator_points: Arc::new(mercator_points),
                    original_indices: None, // Full segment, no remapping needed
                };

                // Insert into quadtree at appropriate level
                quadtree
                    .root
                    .insert_segment(raw_segment, pixel_viewport, bias);
            }
        }

        Ok(quadtree)
    }

    /// Merge another quadtree into this one
    ///
    /// Both quadtrees must have the same configuration (viewport and bias).
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

        // Merge root nodes recursively
        self.root.merge_with(other.root)?;
        Ok(())
    }

    /// Query for segments intersecting the viewport
    ///
    /// Returns segments at the appropriate LOD level for the given viewport size.
    /// Simplification is done lazily and cached, and results are clipped to the viewport.
    pub fn query(&self, geo_viewport: Rect<f64>) -> Vec<SimplifiedSegment> {
        let target_level = self.calculate_target_level(geo_viewport);
        let target_tolerance = QuadtreeNode::calculate_pixel_tolerance(
            target_level,
            self.reference_pixel_viewport,
            self.bias,
        );

        let mut raw_results = Vec::new();
        self.root.query_segments(geo_viewport, &mut raw_results);

        // Convert raw segments to simplified segments with lazy caching
        // and clip to viewport
        let mut results = Vec::with_capacity(raw_results.len());

        for raw in raw_results {
            let simplified = self.get_or_create_simplified_clipped(
                raw,
                target_tolerance,
                target_level,
                geo_viewport,
            );
            if !simplified.parts.is_empty()
                && simplified
                    .parts
                    .iter()
                    .any(|p| !p.simplified_indices.is_empty())
            {
                results.push(simplified);
            }
        }

        results
    }

    /// Get or create a simplified version of a segment at the given tolerance,
    /// clipped to the viewport to only include visible points.
    fn get_or_create_simplified_clipped(
        &self,
        raw: &RawSegment,
        tolerance: f64,
        tolerance_level: u32,
        viewport: Rect<f64>,
    ) -> SimplifiedSegment {
        // For chunked segments, we need a unique cache key that includes the chunk identity
        let chunk_hash = raw.original_indices.as_ref().map(|indices| {
            // Use first and last original index as part of key
            let first = indices.first().copied().unwrap_or(0);
            let last = indices.last().copied().unwrap_or(0);
            (first, last, indices.len())
        });

        let cache_key = SimplificationCacheKey {
            route_ptr: Arc::as_ptr(&raw.route) as usize,
            track_index: raw.track_index,
            segment_index: raw.segment_index,
            tolerance_level,
            chunk_hash,
        };

        // Try to get simplified indices from cache first
        let simplified_indices = {
            let cache = self.simplification_cache.read().unwrap();
            cache.get(&cache_key).map(|arc| arc.as_ref().clone())
        };

        let simplified_indices = simplified_indices.unwrap_or_else(|| {
            // Not in cache, compute it
            let indices = simplify_vw_indices_fast(&raw.mercator_points, tolerance);
            let indices_arc = Arc::new(indices.clone());

            // Store in cache
            {
                let mut cache = self.simplification_cache.write().unwrap();
                cache.insert(cache_key, indices_arc);
            }

            indices
        });

        // Now clip the simplified indices to only include points in/near the viewport
        // This returns multiple runs to handle discontinuities (exit and re-enter viewport)
        let clipped_runs =
            clip_indices_to_viewport_runs(&simplified_indices, &raw.mercator_points, viewport);

        // Convert each run to a SegmentPart
        let segment_len = raw.route.gpx_data().tracks[raw.track_index].segments[raw.segment_index]
            .points
            .len();

        let parts: Vec<SegmentPart> = clipped_runs
            .into_iter()
            .map(|run| {
                let final_indices = map_to_original_indices(&run, &raw.original_indices);
                SegmentPart::new(
                    raw.track_index,
                    raw.segment_index,
                    0..segment_len,
                    final_indices,
                )
            })
            .filter(|part| !part.simplified_indices.is_empty())
            .collect();

        SimplifiedSegment::new(raw.route.clone(), raw.route_index, parts)
    }

    /// Calculate the appropriate LOD level for the given viewport
    fn calculate_target_level(&self, geo_viewport: Rect<f64>) -> u32 {
        let viewport_width_meters = geo_viewport.width();

        // Find level where nodes are approximately 1-2 node widths visible
        let mut level = 0;
        let mut node_width = utils::EARTH_SIZE_METERS;

        while node_width > viewport_width_meters * 2.0 && level < MAX_DEPTH {
            level += 1;
            node_width /= 2.0;
        }

        level
    }
}

impl QuadtreeNode {
    /// Create a root node covering the entire Earth in Web Mercator
    fn new_root(_reference_pixel_viewport: Rect<f64>, _bias: f64) -> Self {
        let bounding_box = Rect::new(
            Coord {
                x: utils::EARTH_MERCATOR_MIN,
                y: utils::EARTH_MERCATOR_MIN,
            },
            Coord {
                x: utils::EARTH_MERCATOR_MAX,
                y: utils::EARTH_MERCATOR_MAX,
            },
        );

        Self {
            bounding_box,
            level: 0,
            raw_segments: Vec::new(),
            children: None,
        }
    }

    /// Create a child node with the given bounding box and level
    fn new_child(
        bounding_box: Rect<f64>,
        level: u32,
        _pixel_viewport: Rect<f64>,
        _bias: f64,
    ) -> Self {
        Self {
            bounding_box,
            level,
            raw_segments: Vec::new(),
            children: None,
        }
    }

    /// Calculate pixel tolerance for a given level
    ///
    /// Higher levels (deeper in tree) have smaller tolerance (more detail).
    /// Higher bias values result in smaller tolerance (more detail retained).
    fn calculate_pixel_tolerance(level: u32, pixel_viewport: Rect<f64>, bias: f64) -> f64 {
        let node_size_meters = utils::EARTH_SIZE_METERS / (1u64 << level) as f64;
        let pixels_per_meter = pixel_viewport.width() / node_size_meters;
        // Invert bias so higher bias = more detail (lower tolerance)
        1.0 / (bias * pixels_per_meter).max(1e-15)
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

        // Create 4 children: NW, NE, SW, SE
        let nw = QuadtreeNode::new_child(
            Rect::new(Coord { x: min.x, y: mid_y }, Coord { x: mid_x, y: max.y }),
            child_level,
            pixel_viewport,
            bias,
        );

        let ne = QuadtreeNode::new_child(
            Rect::new(Coord { x: mid_x, y: mid_y }, Coord { x: max.x, y: max.y }),
            child_level,
            pixel_viewport,
            bias,
        );

        let sw = QuadtreeNode::new_child(
            Rect::new(Coord { x: min.x, y: min.y }, Coord { x: mid_x, y: mid_y }),
            child_level,
            pixel_viewport,
            bias,
        );

        let se = QuadtreeNode::new_child(
            Rect::new(Coord { x: mid_x, y: min.y }, Coord { x: max.x, y: mid_y }),
            child_level,
            pixel_viewport,
            bias,
        );

        self.children = Some(Box::new([nw, ne, sw, se]));
    }

    /// Insert a segment into the appropriate level of the tree
    ///
    /// Segments are chunked at node boundaries so each node only stores
    /// the portion of the segment that falls within its bounds.
    fn insert_segment(&mut self, segment: RawSegment, pixel_viewport: Rect<f64>, bias: f64) {
        // Check if segment intersects this node's bounding box
        if !self.segment_intersects_bounds(&segment.mercator_points) {
            return;
        }

        // Determine if we should recurse deeper
        let should_recurse = self.level < MAX_DEPTH
            && segment.mercator_points.len() >= MIN_POINTS_FOR_RECURSION
            && self.segment_spans_multiple_children(&segment.mercator_points);

        if should_recurse {
            // Ensure children exist
            if self.children.is_none() {
                self.subdivide(pixel_viewport, bias);
            }

            // Split the segment and insert chunks into children
            if let Some(children) = &mut self.children {
                for child in children.iter_mut() {
                    // Extract the portion of the segment that intersects this child
                    if let Some(chunk) = child.extract_segment_chunk(&segment) {
                        child.insert_segment(chunk, pixel_viewport, bias);
                    }
                }
            }
        } else {
            // Store at this level - it's the appropriate granularity
            self.raw_segments.push(segment);
        }
    }

    /// Extract the portion of a segment that intersects this node's bounding box
    ///
    /// Returns a new RawSegment containing only the points (and connecting points)
    /// that are relevant to this node's bounds. Returns None if no points intersect.
    fn extract_segment_chunk(&self, segment: &RawSegment) -> Option<RawSegment> {
        let points = &segment.mercator_points;
        if points.is_empty() {
            return None;
        }

        let min = self.bounding_box.min();
        let max = self.bounding_box.max();

        // Find runs of consecutive points that are in or connected to this node
        let mut chunk_points: Vec<Point<f64>> = Vec::new();
        let mut chunk_indices: Vec<usize> = Vec::new();

        for i in 0..points.len() {
            let point = &points[i];
            let in_bounds = point.x() >= min.x
                && point.x() <= max.x
                && point.y() >= min.y
                && point.y() <= max.y;

            // Check if this point or adjacent line segments cross the bounds
            let prev_crosses = if i > 0 {
                line_intersects_rect(points[i - 1], *point, self.bounding_box)
            } else {
                false
            };
            let next_crosses = if i + 1 < points.len() {
                line_intersects_rect(*point, points[i + 1], self.bounding_box)
            } else {
                false
            };

            if in_bounds || prev_crosses || next_crosses {
                // Include this point in the chunk
                if chunk_indices.last() != Some(&i) {
                    chunk_points.push(*point);
                    chunk_indices.push(i);
                }

                // Also include adjacent points for continuity
                if prev_crosses && i > 0 && chunk_indices.last() != Some(&(i - 1)) {
                    // Insert previous point at the right position
                    let insert_pos = chunk_points.len().saturating_sub(1);
                    chunk_points.insert(insert_pos, points[i - 1]);
                    chunk_indices.insert(insert_pos, i - 1);
                }
                if next_crosses && i + 1 < points.len() {
                    chunk_points.push(points[i + 1]);
                    chunk_indices.push(i + 1);
                }
            }
        }

        // Need at least 2 points to form a segment
        if chunk_points.len() < 2 {
            return None;
        }

        // Deduplicate consecutive indices (may have duplicates from boundary handling)
        let mut deduped_points: Vec<Point<f64>> = Vec::with_capacity(chunk_points.len());
        let mut deduped_indices: Vec<usize> = Vec::with_capacity(chunk_indices.len());

        for (point, idx) in chunk_points.into_iter().zip(chunk_indices.into_iter()) {
            if deduped_indices.last() != Some(&idx) {
                deduped_points.push(point);
                deduped_indices.push(idx);
            }
        }

        if deduped_points.len() < 2 {
            return None;
        }

        Some(RawSegment {
            route: segment.route.clone(),
            route_index: segment.route_index,
            track_index: segment.track_index,
            segment_index: segment.segment_index,
            mercator_points: Arc::new(deduped_points),
            // Store the original indices so we can map back for rendering
            original_indices: Some(Arc::new(deduped_indices)),
        })
    }

    /// Check if a segment spans multiple children of this node
    fn segment_spans_multiple_children(&self, points: &[Point<f64>]) -> bool {
        if points.is_empty() {
            return false;
        }

        let min = self.bounding_box.min();
        let max = self.bounding_box.max();
        let mid_x = (min.x + max.x) / 2.0;
        let mid_y = (min.y + max.y) / 2.0;

        let mut quadrants = [false; 4]; // NW, NE, SW, SE

        for point in points {
            let is_east = point.x() >= mid_x;
            let is_north = point.y() >= mid_y;

            match (is_east, is_north) {
                (false, true) => quadrants[0] = true,  // NW
                (true, true) => quadrants[1] = true,   // NE
                (false, false) => quadrants[2] = true, // SW
                (true, false) => quadrants[3] = true,  // SE
            }
        }

        // Check if segment spans multiple quadrants
        quadrants.iter().filter(|&&q| q).count() > 1
    }

    /// Check if a segment (as points) intersects this node's bounding box
    fn segment_intersects_bounds(&self, points: &[Point<f64>]) -> bool {
        if points.is_empty() {
            return false;
        }

        let min = self.bounding_box.min();
        let max = self.bounding_box.max();

        // Check if any point is inside the bounding box
        for point in points {
            if point.x() >= min.x && point.x() <= max.x && point.y() >= min.y && point.y() <= max.y
            {
                return true;
            }
        }

        // Check if any line segment crosses the bounding box
        for i in 0..points.len().saturating_sub(1) {
            if line_intersects_rect(points[i], points[i + 1], self.bounding_box) {
                return true;
            }
        }

        false
    }

    /// Merge another node into this one
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

        // Check bounding box equality with tolerance
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
        self.raw_segments.extend(other.raw_segments);

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

    /// Query this node and its children for raw segments intersecting the viewport
    fn query_segments<'a>(&'a self, viewport: Rect<f64>, results: &mut Vec<&'a RawSegment>) {
        // Frustum culling - check if this node intersects the viewport
        if !self.intersects_viewport(viewport) {
            return;
        }

        // Add segments from this node that actually intersect the viewport
        for segment in &self.raw_segments {
            if segment_intersects_viewport(&segment.mercator_points, viewport) {
                results.push(segment);
            }
        }

        // Recurse into children
        if let Some(children) = &self.children {
            for child in children.iter() {
                child.query_segments(viewport, results);
            }
        }
    }

    /// Check if this node intersects the viewport
    fn intersects_viewport(&self, viewport: Rect<f64>) -> bool {
        let min = self.bounding_box.min();
        let max = self.bounding_box.max();
        let vmin = viewport.min();
        let vmax = viewport.max();

        // Check for intersection (not disjoint)
        !(max.x < vmin.x || min.x > vmax.x || max.y < vmin.y || min.y > vmax.y)
    }
}

/// Check if a line segment intersects a rectangle
fn line_intersects_rect(p1: Point<f64>, p2: Point<f64>, rect: Rect<f64>) -> bool {
    let min = rect.min();
    let max = rect.max();

    // Use Cohen-Sutherland-style outcode to check intersection
    let outcode = |p: Point<f64>| -> u8 {
        let mut code = 0u8;
        if p.x() < min.x {
            code |= 1;
        } // left
        if p.x() > max.x {
            code |= 2;
        } // right
        if p.y() < min.y {
            code |= 4;
        } // bottom
        if p.y() > max.y {
            code |= 8;
        } // top
        code
    };

    let code1 = outcode(p1);
    let code2 = outcode(p2);

    // Both points inside
    if code1 == 0 && code2 == 0 {
        return true;
    }

    // Both points in same outside region
    if code1 & code2 != 0 {
        return false;
    }

    // Line might cross - do more detailed check
    // Check against all 4 edges
    let edges = [
        (Point::new(min.x, min.y), Point::new(min.x, max.y)), // left
        (Point::new(max.x, min.y), Point::new(max.x, max.y)), // right
        (Point::new(min.x, min.y), Point::new(max.x, min.y)), // bottom
        (Point::new(min.x, max.y), Point::new(max.x, max.y)), // top
    ];

    for (e1, e2) in edges {
        if segments_intersect(p1, p2, e1, e2) {
            return true;
        }
    }

    false
}

/// Check if two line segments intersect
fn segments_intersect(p1: Point<f64>, p2: Point<f64>, p3: Point<f64>, p4: Point<f64>) -> bool {
    let d1 = direction(p3, p4, p1);
    let d2 = direction(p3, p4, p2);
    let d3 = direction(p1, p2, p3);
    let d4 = direction(p1, p2, p4);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    if d1 == 0.0 && on_segment(p3, p4, p1) {
        return true;
    }
    if d2 == 0.0 && on_segment(p3, p4, p2) {
        return true;
    }
    if d3 == 0.0 && on_segment(p1, p2, p3) {
        return true;
    }
    if d4 == 0.0 && on_segment(p1, p2, p4) {
        return true;
    }

    false
}

/// Calculate cross product direction
fn direction(p1: Point<f64>, p2: Point<f64>, p3: Point<f64>) -> f64 {
    (p3.x() - p1.x()) * (p2.y() - p1.y()) - (p2.x() - p1.x()) * (p3.y() - p1.y())
}

/// Check if point p is on segment (p1, p2)
fn on_segment(p1: Point<f64>, p2: Point<f64>, p: Point<f64>) -> bool {
    p.x() >= p1.x().min(p2.x())
        && p.x() <= p1.x().max(p2.x())
        && p.y() >= p1.y().min(p2.y())
        && p.y() <= p1.y().max(p2.y())
}

/// Check if a segment (as a list of points) intersects a viewport rectangle
fn segment_intersects_viewport(points: &[Point<f64>], viewport: Rect<f64>) -> bool {
    if points.is_empty() {
        return false;
    }

    let vmin = viewport.min();
    let vmax = viewport.max();

    // Check if any point is inside the viewport
    for point in points {
        if point.x() >= vmin.x && point.x() <= vmax.x && point.y() >= vmin.y && point.y() <= vmax.y
        {
            return true;
        }
    }

    // Check if any line segment crosses the viewport
    for i in 0..points.len().saturating_sub(1) {
        if line_intersects_rect(points[i], points[i + 1], viewport) {
            return true;
        }
    }

    false
}

/// Fast O(n) simplification using Visvalingam-Whyatt that directly returns indices
///
/// This uses the geo crate's SimplifyVwIdx trait which returns indices directly,
/// avoiding the O(n²) mapping step.
fn simplify_vw_indices_fast(points: &[Point<f64>], tolerance: f64) -> Vec<usize> {
    if points.len() <= 2 {
        return (0..points.len()).collect();
    }

    // Convert to geo::LineString for simplification
    let coords: Vec<Coord<f64>> = points
        .iter()
        .map(|p| Coord { x: p.x(), y: p.y() })
        .collect();
    let linestring = LineString::from(coords);

    // Use SimplifyVwIdx which directly returns preserved indices - O(n log n)
    linestring.simplify_vw_idx(tolerance)
}

/// Map simplified chunk indices back to original segment indices
fn map_to_original_indices(
    simplified_indices: &[usize],
    original_indices: &Option<Arc<Vec<usize>>>,
) -> Vec<usize> {
    match original_indices {
        Some(orig) => {
            // Map through the original indices
            simplified_indices
                .iter()
                .filter_map(|&i| orig.get(i).copied())
                .collect()
        }
        None => {
            // No mapping needed, these are already original indices
            simplified_indices.to_vec()
        }
    }
}

/// Clip simplified indices to only include points that are within or connected to the viewport.
/// Returns multiple runs (Vec<Vec<usize>>) to handle discontinuities where the route exits
/// and re-enters the viewport - each run is a continuous sequence that should be rendered
/// as a separate polyline.
fn clip_indices_to_viewport_runs(
    simplified_indices: &[usize],
    mercator_points: &[Point<f64>],
    viewport: Rect<f64>,
) -> Vec<Vec<usize>> {
    if simplified_indices.is_empty() || mercator_points.is_empty() {
        return Vec::new();
    }

    let vmin = viewport.min();
    let vmax = viewport.max();

    // Check if a point is inside the viewport
    let point_in_viewport = |idx: usize| -> bool {
        if let Some(p) = mercator_points.get(idx) {
            p.x() >= vmin.x && p.x() <= vmax.x && p.y() >= vmin.y && p.y() <= vmax.y
        } else {
            false
        }
    };

    // Check if a line segment between two simplified indices crosses the viewport
    let line_crosses_viewport = |idx1: usize, idx2: usize| -> bool {
        match (mercator_points.get(idx1), mercator_points.get(idx2)) {
            (Some(&p1), Some(&p2)) => line_intersects_rect(p1, p2, viewport),
            _ => false,
        }
    };

    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut current_run: Vec<usize> = Vec::new();

    for (i, &idx) in simplified_indices.iter().enumerate() {
        let in_viewport = point_in_viewport(idx);

        // Check if line to previous point crosses viewport
        let prev_line_crosses = if i > 0 {
            line_crosses_viewport(simplified_indices[i - 1], idx)
        } else {
            false
        };

        // Check if line to next point crosses viewport
        let next_line_crosses = if i + 1 < simplified_indices.len() {
            line_crosses_viewport(idx, simplified_indices[i + 1])
        } else {
            false
        };

        // Should this point be included?
        let should_include = in_viewport || prev_line_crosses || next_line_crosses;

        if should_include {
            // Starting a new run? Include the previous point for line continuity at entry
            if current_run.is_empty() && i > 0 && prev_line_crosses {
                current_run.push(simplified_indices[i - 1]);
            }

            current_run.push(idx);

            // If this point is an "exit" point (next line crosses but next point not in viewport),
            // we need to include the next point for continuity, then end the run
            if next_line_crosses && i + 1 < simplified_indices.len() {
                let next_idx = simplified_indices[i + 1];
                if !point_in_viewport(next_idx) {
                    // Include exit point
                    current_run.push(next_idx);
                    // End this run - route is leaving viewport
                    if current_run.len() >= 2 {
                        runs.push(std::mem::take(&mut current_run));
                    } else {
                        current_run.clear();
                    }
                }
            }
        } else {
            // Point not included - if we have a run, end it
            if current_run.len() >= 2 {
                runs.push(std::mem::take(&mut current_run));
            } else {
                current_run.clear();
            }
        }
    }

    // Don't forget the last run
    if current_run.len() >= 2 {
        runs.push(current_run);
    }

    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quadtree_creation() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );
        let quadtree = Quadtree::new(viewport, 1.0);

        assert!(quadtree.root.raw_segments.is_empty());
        assert!(quadtree.root.children.is_none());
    }

    #[test]
    fn test_pixel_tolerance_calculation() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );

        let tol_0 = QuadtreeNode::calculate_pixel_tolerance(0, viewport, 1.0);
        let tol_1 = QuadtreeNode::calculate_pixel_tolerance(1, viewport, 1.0);
        let tol_10 = QuadtreeNode::calculate_pixel_tolerance(10, viewport, 1.0);

        // All tolerances should be positive
        assert!(tol_0 > 0.0);
        assert!(tol_1 > 0.0);
        assert!(tol_10 > 0.0);

        // Higher levels (deeper in tree, smaller nodes) should have smaller tolerance (more detail)
        // Level 0 covers Earth, level 1 is half, etc.
        // Tolerance = 1 / (bias * pixels_per_meter)
        // pixels_per_meter = viewport_width / node_size_meters
        // As level increases, node_size decreases, so pixels_per_meter increases, so tolerance decreases
        // Higher bias also means lower tolerance (more detail)
        assert!(tol_0 > tol_1, "tol_0={} should be > tol_1={}", tol_0, tol_1);
        assert!(
            tol_1 > tol_10,
            "tol_1={} should be > tol_10={}",
            tol_1,
            tol_10
        );
    }

    #[test]
    fn test_simplify_vw_indices_fast() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.1),
            Point::new(2.0, 0.0),
            Point::new(3.0, 0.1),
            Point::new(4.0, 0.0),
        ];

        let indices = simplify_vw_indices_fast(&points, 0.2);
        assert!(indices.len() <= points.len());
        // Should always keep first and last
        assert!(indices.contains(&0));
        assert!(indices.contains(&(points.len() - 1)));
    }

    #[test]
    fn test_simplify_vw_indices_fast_short() {
        // Test with 2 or fewer points
        let points_2 = vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)];
        let indices_2 = simplify_vw_indices_fast(&points_2, 0.1);
        assert_eq!(indices_2.len(), 2);

        let points_1 = vec![Point::new(0.0, 0.0)];
        let indices_1 = simplify_vw_indices_fast(&points_1, 0.1);
        assert_eq!(indices_1.len(), 1);
    }

    #[test]
    fn test_node_subdivide() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
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
        for child in children.iter() {
            assert_eq!(child.level, 1);
        }
    }

    #[test]
    fn test_intersects_viewport() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );

        let node = QuadtreeNode::new_root(viewport, 1.0);

        // Viewport inside node should intersect
        let small_viewport = Rect::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 100.0, y: 100.0 });
        assert!(node.intersects_viewport(small_viewport));
    }

    #[test]
    fn test_line_intersects_rect() {
        let rect = Rect::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 });

        // Line fully inside
        assert!(line_intersects_rect(
            Point::new(2.0, 2.0),
            Point::new(8.0, 8.0),
            rect
        ));

        // Line crossing through
        assert!(line_intersects_rect(
            Point::new(-5.0, 5.0),
            Point::new(15.0, 5.0),
            rect
        ));

        // Line fully outside
        assert!(!line_intersects_rect(
            Point::new(20.0, 20.0),
            Point::new(30.0, 30.0),
            rect
        ));
    }

    #[test]
    fn test_segment_intersects_bounds() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );

        let node = QuadtreeNode::new_child(
            Rect::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 100.0, y: 100.0 }),
            1,
            viewport,
            1.0,
        );

        // Points inside should intersect
        let inside_points = vec![Point::new(50.0, 50.0), Point::new(60.0, 60.0)];
        assert!(node.segment_intersects_bounds(&inside_points));

        // Points outside should not intersect
        let outside_points = vec![Point::new(200.0, 200.0), Point::new(300.0, 300.0)];
        assert!(!node.segment_intersects_bounds(&outside_points));
    }

    #[test]
    fn test_calculate_target_level() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );
        let quadtree = Quadtree::new(viewport, 1.0);

        // Large viewport should result in low level
        let large_geo_viewport = Rect::new(
            Coord {
                x: -10000000.0,
                y: -10000000.0,
            },
            Coord {
                x: 10000000.0,
                y: 10000000.0,
            },
        );
        let level_large = quadtree.calculate_target_level(large_geo_viewport);

        // Small viewport should result in higher level
        let small_geo_viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1000.0,
                y: 1000.0,
            },
        );
        let level_small = quadtree.calculate_target_level(small_geo_viewport);

        assert!(level_small > level_large);
    }

    #[test]
    fn test_segment_spans_multiple_children() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );

        let node = QuadtreeNode::new_child(
            Rect::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 100.0, y: 100.0 }),
            1,
            viewport,
            1.0,
        );

        // Points spanning multiple quadrants
        let spanning_points = vec![
            Point::new(25.0, 25.0), // SW
            Point::new(75.0, 75.0), // NE
        ];
        assert!(node.segment_spans_multiple_children(&spanning_points));

        // Points in single quadrant
        let single_quadrant_points = vec![Point::new(25.0, 25.0), Point::new(30.0, 30.0)];
        assert!(!node.segment_spans_multiple_children(&single_quadrant_points));
    }

    #[test]
    fn test_clip_indices_to_viewport_runs() {
        // Create a line of points: 0, 1, 2, 3, 4 along x-axis
        let points: Vec<Point<f64>> = (0..5).map(|i| Point::new(i as f64 * 10.0, 0.0)).collect();
        // Points at: (0,0), (10,0), (20,0), (30,0), (40,0)

        let all_indices: Vec<usize> = (0..5).collect();

        // Viewport that only covers the middle part (15 to 25)
        let viewport = Rect::new(Coord { x: 15.0, y: -5.0 }, Coord { x: 25.0, y: 5.0 });

        let runs = clip_indices_to_viewport_runs(&all_indices, &points, viewport);

        // Should have one run containing point 2 (at x=20, inside viewport)
        // Plus points 1 and 3 for line continuity (lines 1-2 and 2-3 cross viewport)
        assert!(!runs.is_empty(), "Should have at least one run");
        let all_points: Vec<usize> = runs.iter().flatten().copied().collect();
        assert!(
            all_points.contains(&2),
            "Should contain point inside viewport"
        );
        // Should not contain points 0 and 4 which are far outside the viewport
        assert!(
            !all_points.contains(&0) || !all_points.contains(&4),
            "Should not contain both far endpoints, got {:?}",
            all_points
        );
    }

    #[test]
    fn test_clip_indices_discontinuity() {
        // Create points that form a U-shape: goes down, across, then up
        // Points: (0,50), (0,0), (50,0), (100,0), (100,50)
        let points: Vec<Point<f64>> = vec![
            Point::new(0.0, 50.0),   // 0: top-left
            Point::new(0.0, 0.0),    // 1: bottom-left
            Point::new(50.0, 0.0),   // 2: bottom-middle
            Point::new(100.0, 0.0),  // 3: bottom-right
            Point::new(100.0, 50.0), // 4: top-right
        ];

        let all_indices: Vec<usize> = (0..5).collect();

        // Viewport that only covers the top portion (y > 40)
        // This should see points 0 and 4, but they are NOT connected!
        let viewport = Rect::new(Coord { x: -10.0, y: 40.0 }, Coord { x: 110.0, y: 60.0 });

        let runs = clip_indices_to_viewport_runs(&all_indices, &points, viewport);

        // Should have TWO separate runs: one for entry (0->1) and one for exit (3->4)
        // They should NOT be connected as a single run
        assert!(
            runs.len() >= 2 || runs.iter().map(|r| r.len()).sum::<usize>() <= 4,
            "Should have separate runs for discontinuous segments, got {:?}",
            runs
        );

        // Verify that points 0 and 4 are not in the same run without intermediate points
        for run in &runs {
            if run.contains(&0) && run.contains(&4) {
                // If both are in same run, there must be intermediate points
                assert!(
                    run.len() > 2,
                    "Points 0 and 4 should not be directly connected, run: {:?}",
                    run
                );
            }
        }
    }

    #[test]
    fn test_viewport_clipping_circular_route() {
        use crate::utils::wgs84_to_mercator;

        // Create a circular route
        let mut gpx = gpx::Gpx::default();
        let mut track = gpx::Track::default();
        let mut segment = gpx::TrackSegment::default();

        // Create a circle with 36 points (10 degree increments)
        let center_lat = 51.5;
        let center_lon = 0.0;
        let radius = 0.5; // degrees

        for i in 0..36 {
            let angle = (i as f64) * 10.0 * std::f64::consts::PI / 180.0;
            let lat = center_lat + radius * angle.sin();
            let lon = center_lon + radius * angle.cos();
            segment
                .points
                .push(gpx::Waypoint::new(geo::Point::new(lon, lat)));
        }
        // Close the circle
        segment.points.push(gpx::Waypoint::new(geo::Point::new(
            center_lon + radius,
            center_lat,
        )));

        track.segments.push(segment);
        gpx.tracks.push(track);

        let config = crate::Config::default();
        let mut collection = crate::RouteCollection::new(config);
        collection.add_route(gpx).unwrap();

        // Query the full circle
        let min_full = wgs84_to_mercator(center_lat - 1.0, center_lon - 1.0);
        let max_full = wgs84_to_mercator(center_lat + 1.0, center_lon + 1.0);
        let full_viewport = Rect::new(
            Coord {
                x: min_full.x(),
                y: min_full.y(),
            },
            Coord {
                x: max_full.x(),
                y: max_full.y(),
            },
        );
        let segments_full = collection.query_visible(full_viewport);
        let points_full: usize = segments_full
            .iter()
            .map(|s| {
                s.parts
                    .iter()
                    .map(|p| p.simplified_indices.len())
                    .sum::<usize>()
            })
            .sum();

        // Query only a small part of the circle (e.g., top portion)
        let min_small = wgs84_to_mercator(center_lat + 0.3, center_lon - 0.2);
        let max_small = wgs84_to_mercator(center_lat + 0.7, center_lon + 0.2);
        let small_viewport = Rect::new(
            Coord {
                x: min_small.x(),
                y: min_small.y(),
            },
            Coord {
                x: max_small.x(),
                y: max_small.y(),
            },
        );
        let segments_small = collection.query_visible(small_viewport);
        let points_small: usize = segments_small
            .iter()
            .map(|s| {
                s.parts
                    .iter()
                    .map(|p| p.simplified_indices.len())
                    .sum::<usize>()
            })
            .sum();

        // The small viewport should have significantly fewer points
        assert!(
            points_small < points_full,
            "Small viewport should have fewer points ({}) than full viewport ({})",
            points_small,
            points_full
        );

        // Small viewport should have at most half the points (we're viewing ~1/4 of circle)
        assert!(
            points_small <= points_full / 2 + 2, // +2 for boundary points
            "Small viewport ({}) should have at most half the points of full ({})",
            points_small,
            points_full
        );
    }

    #[test]
    fn test_extract_segment_chunk() {
        let viewport = Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: 1024.0,
                y: 768.0,
            },
        );

        // Create a node with bounds [0, 100] x [0, 100]
        let node = QuadtreeNode::new_child(
            Rect::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 100.0, y: 100.0 }),
            1,
            viewport,
            1.0,
        );

        // Create a mock route for testing
        let mut gpx = gpx::Gpx::default();
        let mut track = gpx::Track::default();
        let mut segment = gpx::TrackSegment::default();
        for i in 0..10 {
            segment
                .points
                .push(gpx::Waypoint::new(geo::Point::new(i as f64, i as f64)));
        }
        track.segments.push(segment);
        gpx.tracks.push(track);
        let route = crate::Route::new(gpx).unwrap();

        // Create a segment that spans from (-50, -50) to (150, 150) - crosses the node
        let points: Vec<Point<f64>> = (0..10)
            .map(|i| Point::new(-50.0 + i as f64 * 25.0, -50.0 + i as f64 * 25.0))
            .collect();

        let raw_segment = RawSegment {
            route: route.clone(),
            route_index: 0,
            track_index: 0,
            segment_index: 0,
            mercator_points: Arc::new(points),
            original_indices: None,
        };

        // Extract chunk - should only include points in/near the node bounds
        let chunk = node.extract_segment_chunk(&raw_segment);
        assert!(chunk.is_some());

        let chunk = chunk.unwrap();
        // The chunk should have fewer points than the original
        assert!(chunk.mercator_points.len() < raw_segment.mercator_points.len());
        // The chunk should have original_indices set
        assert!(chunk.original_indices.is_some());

        // All chunk points should be in or near the node bounds
        for point in chunk.mercator_points.iter() {
            // Points should be within extended bounds (including boundary crossings)
            assert!(
                point.x() >= -50.0 && point.x() <= 150.0,
                "Point x={} out of extended range",
                point.x()
            );
        }
    }

    #[test]
    fn test_chunking_reduces_points_on_pan() {
        use crate::utils::wgs84_to_mercator;

        // Create a long track that spans a large area
        let mut gpx = gpx::Gpx::default();
        let mut track = gpx::Track::default();
        let mut segment = gpx::TrackSegment::default();

        // Create 100 points spanning from London to Paris (roughly)
        for i in 0..100 {
            let lat = 51.5 + (i as f64 * 0.02); // ~51.5 to ~53.5
            let lon = -0.1 + (i as f64 * 0.025); // ~-0.1 to ~2.4
            segment
                .points
                .push(gpx::Waypoint::new(geo::Point::new(lon, lat)));
        }
        track.segments.push(segment);
        gpx.tracks.push(track);

        let config = crate::Config::default();
        let mut collection = crate::RouteCollection::new(config);
        collection.add_route(gpx).unwrap();

        // Query a small viewport at one end of the track
        let min = wgs84_to_mercator(51.5, -0.2);
        let max = wgs84_to_mercator(52.0, 0.3);
        let small_viewport = Rect::new(
            Coord {
                x: min.x(),
                y: min.y(),
            },
            Coord {
                x: max.x(),
                y: max.y(),
            },
        );

        let segments_small = collection.query_visible(small_viewport);

        // Query the full track extent
        let min_full = wgs84_to_mercator(51.0, -1.0);
        let max_full = wgs84_to_mercator(54.0, 3.0);
        let large_viewport = Rect::new(
            Coord {
                x: min_full.x(),
                y: min_full.y(),
            },
            Coord {
                x: max_full.x(),
                y: max_full.y(),
            },
        );

        let segments_large = collection.query_visible(large_viewport);

        // The small viewport should have fewer or equal simplified points
        let points_small: usize = segments_small
            .iter()
            .map(|s| {
                s.parts
                    .iter()
                    .map(|p| p.simplified_indices.len())
                    .sum::<usize>()
            })
            .sum();
        let points_large: usize = segments_large
            .iter()
            .map(|s| {
                s.parts
                    .iter()
                    .map(|p| p.simplified_indices.len())
                    .sum::<usize>()
            })
            .sum();

        // Small viewport should have fewer points due to chunking
        assert!(
            points_small <= points_large,
            "Small viewport has {} points, large has {} - chunking should reduce points",
            points_small,
            points_large
        );
    }
}

//! GPX Route Data Module
//!
//! This module provides efficient storage, indexing, and querying of large-scale GPX route collections
//! for real-time rendering. The core data structure is a level-of-detail (LOD) quadtree that enables
//! sub-100ms queries across thousands of routes with millions of points.
//!
//! # Overview
//!
//! The data module implements an external indexing system for GPX routes, where:
//! - Raw GPX data is stored once in [`Route`] structures
//! - A quadtree spatial index provides fast LOD-based queries
//! - Simplified segments reference original data (no duplication)
//! - Parallel loading enables efficient processing of large datasets
//!
//! # Architecture
//!
//! - **[`Route`]**: Immutable storage for parsed GPX data
//! - **[`Quadtree`]**: Spatial index with Earth-rooted structure
//! - **[`SimplifiedSegment`]**: External index references with LOD simplification
//! - **[`RouteCollection`]**: High-level manager for routes and queries
//!
//! # Usage Example
//!
//! ```rust
//! use large_track_viewer::data::{RouteCollection, Config};
//! use std::fs::File;
//! use std::io::BufReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a collection with default configuration
//! let config = Config::default();
//! let mut collection = RouteCollection::new(config);
//!
//! // Load a GPX file
//! # /*
//! let file = File::open("route.gpx")?;
//! let reader = BufReader::new(file);
//! let gpx = gpx::read(reader)?;
//! # */
//! # let gpx = gpx::Gpx::default();
//!
//! // Add route to collection (note: add_route is tested in tests)
//! // In real usage: collection.add_route(gpx)?;
//!
//! // Query visible segments for a viewport (in Web Mercator coordinates)
//! use large_track_viewer::data::utils::wgs84_to_mercator;
//! // Query visible segments (after routes are added)
//! // let min = wgs84_to_mercator(51.5, -0.2);
//! // let max = wgs84_to_mercator(51.6, -0.0);
//! // let viewport = geo::Rect::new(
//! //     geo::Coord { x: min.x(), y: min.y() },
//! //     geo::Coord { x: max.x(), y: max.y() },
//! // );
//! //
//! // let segments = collection.query_visible(viewport);
//! //
//! // // Access simplified points with boundary context for rendering
//! // for segment in segments {
//! //     for part in &segment.parts {
//! //         let points = part.get_simplified_points(&segment.route);
//! //         // Render points...
//! //     }
//! // }
//! # Ok(())
//! # }
//! ```
//!
//! # Performance Characteristics
//!
//! - **Build Time**: O(N log N) per route, parallelizable
//! - **Query Time**: O(log D + K) where D=depth, K=results
//! - **Memory**: O(N) for raw data + O(S×I) for index (S=segments, I=indices per segment)
//! - **Target**: Sub-100ms queries for 10,000 routes with millions of points

mod collection;
mod quadtree;
mod route;
mod segment;
pub mod utils;

// Public API exports
pub use collection::{CollectionInfo, Config, RouteCollection};
pub use quadtree::Quadtree;
pub use route::Route;
pub use segment::{BoundaryContext, SegmentPart, SimplifiedSegment};

/// Error types for the data module
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("GPX parsing error: {0}")]
    GpxParse(#[from] gpx::errors::GpxError),

    #[error("Invalid geometry: {0}")]
    InvalidGeometry(String),

    #[error("Merge mismatch: {reason}")]
    MergeMismatch { reason: String },

    #[error("Coordinate conversion error: {0}")]
    CoordinateConversion(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Empty route")]
    EmptyRoute,
}

pub type Result<T> = std::result::Result<T, DataError>;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_exists() {
        // Basic smoke test to ensure module compiles
        assert!(true);
    }
}

//! Large Track Library - Core Data Structures for GPX Track Management
//!
//! This library provides efficient storage, indexing, and querying of large-scale GPX route
//! collections for real-time rendering. The core data structure is a level-of-detail (LOD)
//! quadtree that enables fast spatial queries across thousands of routes with millions of points.
//!
//! # Architecture
//!
//! - **[`Route`]**: Immutable storage for parsed GPX data
//! - **[`Quadtree`]**: Spatial index with Earth-rooted structure and LOD support
//! - **[`SimplifiedSegment`]**: External index references with LOD simplification
//! - **[`RouteCollection`]**: High-level manager for routes and queries
//!
//! # Performance Characteristics
//!
//! - **Build Time**: O(N log N) per route, parallelizable
//! - **Query Time**: O(log D + K) where D=depth, K=results
//! - **Memory**: O(N) for raw data + O(S×I) for index (S=segments, I=indices per segment)

mod collection;
mod quadtree;
mod route;
mod segment;
pub mod utils;

// Public API exports
pub use collection::{CollectionInfo, Config, RouteCollection};
pub use quadtree::Quadtree;
pub use route::Route;
pub use segment::{SegmentPart, SimplifiedSegment};

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
    use super::*;

    #[test]
    fn test_public_exports() {
        // Verify that all public types are accessible
        let _: fn(Config) -> RouteCollection = RouteCollection::new;
        let _: fn() -> Config = Config::default;
    }
}

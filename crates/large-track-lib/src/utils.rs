//! Utility functions for coordinate conversions and spatial operations

use geo::Point;

/// Web Mercator bounds in meters (EPSG:3857)
pub const EARTH_MERCATOR_MAX: f64 = 20037508.34;
pub const EARTH_MERCATOR_MIN: f64 = -20037508.34;
pub const EARTH_SIZE_METERS: f64 = EARTH_MERCATOR_MAX - EARTH_MERCATOR_MIN;

/// Maximum latitude that can be represented in Web Mercator
pub const MAX_LATITUDE: f64 = 85.05112878;

/// Precomputed constant: EARTH_MERCATOR_MAX / 180.0
const LON_TO_X_FACTOR: f64 = EARTH_MERCATOR_MAX / 180.0;

/// Precomputed constant: EARTH_MERCATOR_MAX / PI
const Y_FACTOR: f64 = EARTH_MERCATOR_MAX / std::f64::consts::PI;

/// Precomputed constant: 180.0 / EARTH_MERCATOR_MAX
const X_TO_LON_FACTOR: f64 = 180.0 / EARTH_MERCATOR_MAX;

/// Precomputed constant: PI / EARTH_MERCATOR_MAX
const Y_TO_LAT_FACTOR: f64 = std::f64::consts::PI / EARTH_MERCATOR_MAX;

/// Convert WGS84 (lat, lon) to Web Mercator (x, y) in meters
///
/// # Arguments
/// * `lat` - Latitude in degrees (-85.05 to 85.05)
/// * `lon` - Longitude in degrees (-180 to 180)
///
/// # Returns
/// A `Point<f64>` with x (easting) and y (northing) in meters
#[inline(always)]
pub fn wgs84_to_mercator(lat: f64, lon: f64) -> Point<f64> {
    // Clamp latitude to valid Web Mercator range
    let lat = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE);

    let x = lon * LON_TO_X_FACTOR;

    // Optimized: compute lat_rad once
    let lat_rad = lat.to_radians();
    let y = (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() * Y_FACTOR;

    Point::new(x, y)
}

/// Convert WGS84 to Web Mercator without clamping (for trusted input)
///
/// Use this when you know the latitude is already within valid bounds.
#[inline(always)]
pub fn wgs84_to_mercator_unclamped(lat: f64, lon: f64) -> Point<f64> {
    let x = lon * LON_TO_X_FACTOR;
    let lat_rad = lat.to_radians();
    let y = (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() * Y_FACTOR;
    Point::new(x, y)
}

/// Convert Web Mercator (x, y) in meters to WGS84 (lat, lon)
///
/// # Arguments
/// * `x` - Easting in meters
/// * `y` - Northing in meters
///
/// # Returns
/// A tuple of (latitude, longitude) in degrees
#[inline(always)]
pub fn mercator_to_wgs84(x: f64, y: f64) -> (f64, f64) {
    let lon = x * X_TO_LON_FACTOR;
    let lat =
        (std::f64::consts::PI / 2.0 - 2.0 * ((-y * Y_TO_LAT_FACTOR).exp()).atan()).to_degrees();
    (lat, lon)
}

/// Convert a GPX waypoint to Web Mercator point
#[inline(always)]
pub fn waypoint_to_mercator(waypoint: &gpx::Waypoint) -> Point<f64> {
    wgs84_to_mercator(waypoint.point().y(), waypoint.point().x())
}

/// Check if a point is within Web Mercator bounds
#[inline(always)]
pub fn is_valid_mercator(point: &Point<f64>) -> bool {
    let x = point.x();
    let y = point.y();
    x >= EARTH_MERCATOR_MIN
        && x <= EARTH_MERCATOR_MAX
        && y >= EARTH_MERCATOR_MIN
        && y <= EARTH_MERCATOR_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wgs84_to_mercator_origin() {
        let point = wgs84_to_mercator(0.0, 0.0);
        assert!((point.x() - 0.0).abs() < 0.01);
        assert!((point.y() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_wgs84_to_mercator_bounds() {
        let west = wgs84_to_mercator(0.0, -180.0);
        assert!((west.x() - EARTH_MERCATOR_MIN).abs() < 1.0);

        let east = wgs84_to_mercator(0.0, 180.0);
        assert!((east.x() - EARTH_MERCATOR_MAX).abs() < 1.0);
    }

    #[test]
    fn test_mercator_to_wgs84_roundtrip() {
        let lat = 51.5074;
        let lon = -0.1278;

        let mercator = wgs84_to_mercator(lat, lon);
        let (lat2, lon2) = mercator_to_wgs84(mercator.x(), mercator.y());

        assert!((lat - lat2).abs() < 0.0001);
        assert!((lon - lon2).abs() < 0.0001);
    }

    #[test]
    fn test_is_valid_mercator() {
        assert!(is_valid_mercator(&Point::new(0.0, 0.0)));
        assert!(is_valid_mercator(&Point::new(
            EARTH_MERCATOR_MAX,
            EARTH_MERCATOR_MAX
        )));
        assert!(!is_valid_mercator(&Point::new(
            EARTH_MERCATOR_MAX + 1.0,
            0.0
        )));
    }

    #[test]
    fn test_unclamped_matches_clamped_for_valid_input() {
        let lat = 45.0;
        let lon = -90.0;
        let clamped = wgs84_to_mercator(lat, lon);
        let unclamped = wgs84_to_mercator_unclamped(lat, lon);
        assert!((clamped.x() - unclamped.x()).abs() < f64::EPSILON);
        assert!((clamped.y() - unclamped.y()).abs() < f64::EPSILON);
    }
}

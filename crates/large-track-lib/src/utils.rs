//! Utility functions for coordinate conversions and spatial operations

use geo::Point;

/// Web Mercator bounds in meters (EPSG:3857)
pub const EARTH_MERCATOR_MAX: f64 = 20037508.34;
pub const EARTH_MERCATOR_MIN: f64 = -20037508.34;
pub const EARTH_SIZE_METERS: f64 = EARTH_MERCATOR_MAX - EARTH_MERCATOR_MIN;

/// Maximum latitude that can be represented in Web Mercator
pub const MAX_LATITUDE: f64 = 85.05112878;

/// Convert WGS84 (lat, lon) to Web Mercator (x, y) in meters
///
/// # Arguments
/// * `lat` - Latitude in degrees (-85.05 to 85.05)
/// * `lon` - Longitude in degrees (-180 to 180)
///
/// # Returns
/// A `Point<f64>` with x (easting) and y (northing) in meters
pub fn wgs84_to_mercator(lat: f64, lon: f64) -> Point<f64> {
    // Clamp latitude to valid Web Mercator range
    let lat = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE);

    let x = lon * EARTH_MERCATOR_MAX / 180.0;
    let y = (lat.to_radians().tan() + (1.0 / lat.to_radians().cos())).ln() * EARTH_MERCATOR_MAX
        / std::f64::consts::PI;

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
pub fn mercator_to_wgs84(x: f64, y: f64) -> (f64, f64) {
    let lon = (x / EARTH_MERCATOR_MAX) * 180.0;
    let lat = (std::f64::consts::PI / 2.0
        - 2.0 * ((-y / EARTH_MERCATOR_MAX * std::f64::consts::PI).exp()).atan())
    .to_degrees();

    (lat, lon)
}

/// Convert a GPX waypoint to Web Mercator point
pub fn waypoint_to_mercator(waypoint: &gpx::Waypoint) -> Point<f64> {
    wgs84_to_mercator(waypoint.point().y(), waypoint.point().x())
}

/// Check if a point is within Web Mercator bounds
pub fn is_valid_mercator(point: &Point<f64>) -> bool {
    point.x() >= EARTH_MERCATOR_MIN
        && point.x() <= EARTH_MERCATOR_MAX
        && point.y() >= EARTH_MERCATOR_MIN
        && point.y() <= EARTH_MERCATOR_MAX
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
}

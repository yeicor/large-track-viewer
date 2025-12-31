//! Performance benchmarks for large-track-lib
//!
//! Run with: cargo bench --package large-track-lib
//!
//! Reduced benchmark suite for faster iteration during optimization.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use geo::{Coord, Point, Rect};
use gpx::{Gpx, Track, TrackSegment, Waypoint};
use large_track_lib::{Config, RouteCollection};

/// Generate a realistic GPX track with the specified number of points.
fn generate_gpx_track(num_points: usize, base_lat: f64, base_lon: f64) -> Gpx {
    let mut gpx = Gpx::default();
    let mut track = Track::default();
    let mut segment = TrackSegment::default();

    for i in 0..num_points {
        let t = i as f64 / num_points as f64;
        let lat = base_lat + t * 0.1 + (t * 50.0).sin() * 0.001;
        let lon = base_lon + t * 0.1 + (t * 30.0).cos() * 0.001;
        segment.points.push(Waypoint::new(Point::new(lon, lat)));
    }

    track.segments.push(segment);
    gpx.tracks.push(track);
    gpx
}

/// Generate multiple GPX tracks spread across an area
fn generate_multiple_tracks(num_tracks: usize, points_per_track: usize) -> Vec<Gpx> {
    (0..num_tracks)
        .map(|i| {
            let lat_offset = (i % 10) as f64 * 0.1;
            let lon_offset = (i / 10) as f64 * 0.1;
            generate_gpx_track(points_per_track, 51.5 + lat_offset, -0.1 + lon_offset)
        })
        .collect()
}

/// Convert WGS84 to Web Mercator
fn wgs84_to_mercator(lat: f64, lon: f64) -> Point<f64> {
    const EARTH_MERCATOR_MAX: f64 = 20037508.34;
    const MAX_LATITUDE: f64 = 85.05112878;

    let lat = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE);
    let x = lon * EARTH_MERCATOR_MAX / 180.0;
    let y = (lat.to_radians().tan() + (1.0 / lat.to_radians().cos())).ln() * EARTH_MERCATOR_MAX
        / std::f64::consts::PI;
    Point::new(x, y)
}

/// Create a viewport in Web Mercator coordinates
fn create_viewport(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> Rect<f64> {
    let min = wgs84_to_mercator(min_lat, min_lon);
    let max = wgs84_to_mercator(max_lat, max_lon);
    Rect::new(
        Coord {
            x: min.x(),
            y: min.y(),
        },
        Coord {
            x: max.x(),
            y: max.y(),
        },
    )
}

// ============================================================================
// Core Benchmarks - Key performance indicators
// ============================================================================

fn bench_query_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("query");

    // Single route with 50k points - representative workload
    let gpx = generate_gpx_track(50_000, 51.5, -0.1);
    let config = Config::default();
    let mut collection = RouteCollection::new(config);
    collection.add_route(gpx).unwrap();

    // Small viewport (detailed view)
    let small_viewport = create_viewport(51.50, -0.11, 51.51, -0.10);
    group.bench_function("small_viewport_50k", |b| {
        b.iter(|| collection.query_visible(small_viewport));
    });

    // Large viewport (overview)
    let large_viewport = create_viewport(50.0, -2.0, 53.0, 1.0);
    group.bench_function("large_viewport_50k", |b| {
        b.iter(|| collection.query_visible(large_viewport));
    });

    group.finish();
}

fn bench_many_routes(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_routes");
    group.sample_size(20);

    // 100 routes with 1000 points each
    let tracks = generate_multiple_tracks(100, 1_000);
    let config = Config::default();
    let mut collection = RouteCollection::new(config);
    collection.add_routes_parallel(tracks).unwrap();

    let viewport = create_viewport(51.0, -0.5, 52.5, 1.0);
    let total_points = 100 * 1_000;

    group.throughput(Throughput::Elements(total_points as u64));
    group.bench_function("100_routes_1k_each", |b| {
        b.iter(|| collection.query_visible(viewport));
    });

    group.finish();
}

fn bench_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("construction");
    group.sample_size(20);

    // Parallel construction benchmark
    let tracks = generate_multiple_tracks(50, 1_000);
    let total_points = 50 * 1_000;

    group.throughput(Throughput::Elements(total_points as u64));
    group.bench_function("parallel_50x1k", |b| {
        let config = Config::default();
        b.iter(|| {
            let mut collection = RouteCollection::new(config.clone());
            collection.add_routes_parallel(tracks.clone()).unwrap();
        });
    });

    group.finish();
}

fn bench_collection_info(c: &mut Criterion) {
    let mut group = c.benchmark_group("info");

    let tracks = generate_multiple_tracks(100, 1_000);
    let config = Config::default();
    let mut collection = RouteCollection::new(config);
    collection.add_routes_parallel(tracks).unwrap();

    group.bench_function("get_info", |b| {
        b.iter(|| collection.get_info());
    });

    group.bench_function("bounding_box", |b| {
        b.iter(|| collection.bounding_box_wgs84());
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    benches,
    bench_query_performance,
    bench_many_routes,
    bench_construction,
    bench_collection_info,
);

criterion_main!(benches);

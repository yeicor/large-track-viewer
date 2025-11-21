# Large Track Viewer

A high-performance, cross-platform application for viewing and analyzing large GPS track collections with intelligent level-of-detail rendering.

## ✨ Features

### 🗺️ Map Rendering
- **Interactive Map**: Built on [walkers](https://github.com/podusowski/walkers) with smooth pan and zoom
- **Multiple Tile Providers**: OpenStreetMap, OpenTopoMap, CyclOSM support
- **Real-time Track Rendering**: Efficient rendering of GPX tracks on the map

### ⚡ High Performance
- **Quadtree Spatial Index**: Earth-rooted adaptive quadtree for fast spatial queries
- **Level-of-Detail (LOD) System**: Automatic simplification based on zoom level
- **Sub-100ms Queries**: Target performance for millions of track points
- **Parallel Loading**: Multi-threaded GPX file processing
- **External Indexing**: No point duplication, minimal memory overhead

### 📊 Data Management
- **GPX File Support**: Load and display standard GPX 1.1 files
- **Multiple Routes**: Load and view thousands of routes simultaneously
- **Statistics Dashboard**: Real-time stats on routes, points, distances, and query performance
- **Boundary Context**: Smooth line rendering at viewport edges

### 🎨 Customization
- **Adjustable LOD Bias**: Control detail level (0.1-10.0 range)
- **Track Styling**: Customize line width and color
- **Debug Visualization**: Optional boundary context markers

### 🖥️ Cross-Platform
- **Desktop**: Linux, Windows, macOS (native performance)
- **Web**: WASM-based browser support
- **Mobile**: Android support (iOS planned)

## 🚀 Quick Start

### Installation

Download the latest release for your platform from the [releases page](https://github.com/yeicor/large-track-viewer/releases).

### Running from Source

```bash
# Clone the repository
git clone https://github.com/yeicor/large-track-viewer
cd large-track-viewer

# Run in development mode
cargo run --release -- --help
```

### Basic Usage

```bash
# Load a single GPX file
large-track-viewer --gpx-files path/to/route.gpx

# Load multiple files with custom settings
large-track-viewer \
  --gpx-files route1.gpx route2.gpx route3.gpx \
  --bias 2.0 \
  --line-width 3.0 \
  --track-color FF0000

# Set initial map position
large-track-viewer \
  --gpx-files route.gpx \
  --center-lat 51.5074 \
  --center-lon -0.1278 \
  --zoom 12
```

## 📖 CLI Arguments

| Argument | Description | Default |
|----------|-------------|---------|
| `--gpx-files <FILE>...` | GPX files to load on startup | None |
| `--bias <FLOAT>` | LOD bias (higher = more detail) | 1.0 |
| `--max-points-per-node <INT>` | Quadtree subdivision threshold | 100 |
| `--reference-viewport-width <INT>` | Reference viewport width (px) | 1920 |
| `--reference-viewport-height <INT>` | Reference viewport height (px) | 1080 |
| `--center-lat <FLOAT>` | Initial map center latitude | None |
| `--center-lon <FLOAT>` | Initial map center longitude | None |
| `--zoom <INT>` | Initial zoom level (0-18) | 12 |
| `--line-width <FLOAT>` | Track line width in pixels | 2.0 |
| `--track-color <HEX>` | Track color (hex format) | 0000FF |

## 🎮 Usage

### Map Controls
- **Left Click + Drag**: Pan the map
- **Mouse Wheel**: Zoom in/out
- **Double Click**: Zoom in
- **F1**: Toggle help overlay

### UI Panels

#### Files Panel (Left)
- **Load GPX File**: Open file picker to add tracks
- **Clear All**: Remove all loaded tracks
- **Progress**: View loading status and errors

#### Settings Panel (Left)
- **Display**: Adjust line width and track color
- **Level of Detail**: Change LOD bias (requires reload)
- **Map Tiles**: Select tile provider
- **Debug**: Enable boundary context visualization

#### Statistics Panel (Right)
- **Data Overview**: Routes, points, and total distance
- **Performance**: Query times and segments rendered
- **Viewport**: Current map bounds

## 🏗️ Architecture

### Data Module (`src/data/`)

The core data management system with the following components:

#### Route (`route.rs`)
- Immutable storage for parsed GPX data
- Bounding box computation in Web Mercator
- Haversine distance calculations

#### Quadtree (`quadtree.rs`)
- Earth-rooted adaptive spatial index
- No fixed depth limit
- Pixel-based LOD computation at build time
- Fast viewport queries (O(log D + K))

#### Simplified Segment (`segment.rs`)
- External indexing with no point duplication
- Stores indices into original GPX data
- Boundary context for smooth rendering

#### Route Collection (`collection.rs`)
- High-level manager for multiple routes
- Parallel loading and quadtree building
- Fast merging of per-route indices

### App Module (`src/app/`)

The application layer integrating the map UI:

#### Plugin (`plugin.rs`)
- Custom walkers plugin for track rendering
- Viewport-based query execution
- Screen-space coordinate projection

#### State (`state.rs`)
- Application state management
- Route collection wrapper
- UI settings and statistics

#### UI Panels (`ui_panels.rs`)
- Reusable egui components
- Settings, statistics, and file management
- Help overlay

## 🔬 Technical Details

### LOD System

The level-of-detail system uses:
1. **Pixel Tolerance**: `bias / pixels_per_meter`
2. **Visvalingam-Whyatt**: For line simplification
3. **Adaptive Subdivision**: Based on point density
4. **Precomputed Simplifications**: Stored as indices

### Performance Characteristics

- **Build Time**: O(N log N) per route, parallelizable
- **Query Time**: O(log D + K) where D=depth, K=results  
- **Memory**: O(N) raw + O(S×I) index (S=segments, I=indices)
- **Target**: <100ms queries for 10K routes with millions of points

### Coordinate Systems

- **Input**: WGS84 (latitude/longitude)
- **Index**: Web Mercator EPSG:3857 (meters)
- **Rendering**: Screen space (pixels)

## 🛠️ Development

### Project Structure

```
large-track-viewer/
├── src/
│   ├── app/           # Application UI and integration
│   │   ├── mod.rs     # Main app structure
│   │   ├── plugin.rs  # Walkers track rendering plugin
│   │   ├── state.rs   # State management
│   │   ├── ui_panels.rs # UI components
│   │   └── settings.rs  # CLI settings
│   ├── data/          # Core data module
│   │   ├── mod.rs     # Public API
│   │   ├── route.rs   # GPX route storage
│   │   ├── segment.rs # Simplified segments
│   │   ├── quadtree.rs # Spatial index
│   │   ├── collection.rs # Route manager
│   │   └── utils.rs   # Coordinate transforms
│   ├── entrypoints/   # Platform entry points
│   └── lib.rs         # Library root
├── Cargo.toml
└── README.md
```

### Building for Different Platforms

#### Desktop (Native)
```bash
cargo build --release --features native
```

#### Web (WASM)
```bash
trunk build --release
trunk serve  # For development
```

#### Android
```bash
# Requires Android SDK and cargo-ndk
cd android
./gradlew assembleRelease
```

### Running Tests

```bash
# Run all tests
cargo test

# Run data module tests only
cargo test --lib data

# Run with profiling
cargo test --features profiling
```

## 📝 License

This project is dual-licensed under:
- MIT License
- Apache License 2.0

Choose the license that best suits your needs.

## 🤝 Contributing

Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Submit a pull request

## 🙏 Acknowledgments

- [walkers](https://github.com/podusowski/walkers) - Map widget for egui
- [egui](https://github.com/emilk/egui) - Immediate mode GUI framework
- [gpx](https://github.com/georust/gpx) - GPX parsing library
- [geo](https://github.com/georust/geo) - Geospatial algorithms

## 📚 Further Reading

- [Data Module Design](src/data/README.md) - Detailed architecture
- [Previous Design Discussion](https://github.com/yeicor/large-track-viewer/discussions) - Background and requirements

## 🐛 Known Issues

- MapMemory position persistence not yet implemented
- Dynamic tile provider switching requires app restart
- Web file picker not yet implemented
- iOS support planned but not yet available

## 🗺️ Roadmap

- [ ] Persistent quadtree caching to disk
- [ ] Incremental route updates
- [ ] GPU-accelerated simplification
- [ ] Streaming for massive files
- [ ] Route editing capabilities
- [ ] Export simplified tracks
- [ ] Statistical analysis tools
- [ ] Heatmap visualization

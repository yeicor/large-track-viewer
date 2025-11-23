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

The project is organized as a Cargo workspace with three reusable crates:

```
large-track-viewer/
├── crates/
│   ├── large-track-data/           # 📦 Reusable data structures crate
│   │   ├── src/
│   │   │   ├── lib.rs              # Public API
│   │   │   ├── route.rs            # GPX route storage
│   │   │   ├── segment.rs          # Simplified segments
│   │   │   ├── quadtree.rs         # Spatial index
│   │   │   ├── collection.rs       # Route manager
│   │   │   └── utils.rs            # Coordinate transforms
│   │   ├── Cargo.toml
│   │   └── README.md
│   │
│   ├── egui-eframe-entrypoints/    # 📦 Reusable entrypoints crate
│   │   ├── src/
│   │   │   ├── lib.rs              # Cross-platform entry points
│   │   │   ├── cli.rs              # CLI/URL argument parsing
│   │   │   ├── profiling.rs        # Profiling integration
│   │   │   ├── metadata.rs         # Build version info
│   │   │   ├── run.rs              # Generic app runner
│   │   │   └── web.rs              # Web-specific code
│   │   ├── build.rs                # shadow-rs build metadata
│   │   ├── Cargo.toml
│   │   └── README.md
│   │
│   └── large-track-viewer/         # 📦 Main application crate
│       ├── src/
│       │   ├── app/                # Application UI and logic
│       │   │   ├── mod.rs          # Main app structure
│       │   │   ├── plugin.rs       # Walkers track rendering plugin
│       │   │   ├── state.rs        # State management
│       │   │   ├── ui_panels.rs    # UI components
│       │   │   └── settings.rs     # CLI settings
│       │   ├── lib.rs              # Library root
│       │   └── main.rs             # Binary entry point
│       └── Cargo.toml
│
├── Cargo.toml                      # Workspace root
└── README.md
```

### Crate Overview

#### `large-track-data`
A standalone, reusable library for efficient GPX track storage and querying:
- Quadtree spatial indexing with LOD support
- Parallel loading and processing
- Web Mercator coordinate system
- Can be used in any Rust project needing GPX track management

#### `egui-eframe-entrypoints`
A generic, reusable entry points system for egui/eframe apps:
- Cross-platform support (native, web, Android)
- CLI argument parsing (native) and URL query parsing (web)
- Profiling integration with puffin
- Build metadata display
- Can be used by any egui/eframe application

#### `large-track-viewer`
The main application that ties everything together:
- Uses `large-track-data` for GPX track management
- Uses `egui-eframe-entrypoints` for cross-platform entry points
- Implements the UI, map integration, and user interactions
```

### Building for Different Platforms

The workspace structure allows you to build individual crates or the entire workspace:

#### Desktop (Native)
```bash
# Build the entire workspace
cargo build

# Build only the main app
cargo build -p large-track-viewer

# Build with release optimizations
cargo build --release
```

#### Web (WASM)
```bash
# Using trunk (build system for WASM)
trunk build --release
trunk serve  # For development with hot reload
```

#### Android
```bash
# Requires Android SDK and cargo-ndk
cd android
./gradlew assembleRelease
```

#### Build Individual Crates
```bash
# Build just the data structures library
cargo build -p large-track-data

# Build just the entrypoints library
cargo build -p egui-eframe-entrypoints
```

### Running Tests

```bash
# Run all tests in the workspace
cargo test

# Run tests for a specific crate
cargo test -p large-track-data
cargo test -p egui-eframe-entrypoints
cargo test -p large-track-viewer

# Run with profiling enabled
cargo test --features profiling
```

### Using the Reusable Crates

Both `large-track-data` and `egui-eframe-entrypoints` are designed to be reusable in other projects:

#### Using `large-track-data` in your project
```toml
[dependencies]
large-track-data = { git = "https://github.com/yeicor/large-track-viewer", package = "large-track-data" }
```

#### Using `egui-eframe-entrypoints` in your project
```toml
[dependencies]
egui-eframe-entrypoints = { git = "https://github.com/yeicor/large-track-viewer", package = "egui-eframe-entrypoints" }
```

See each crate's README for detailed usage instructions:
- [`large-track-data/README.md`](crates/large-track-data/README.md)
- [`egui-eframe-entrypoints/README.md`](crates/egui-eframe-entrypoints/README.md)
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

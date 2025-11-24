//! UI panels for the application
//!
//! This module provides reusable UI components for the new clean sidebar design
//! with tabs, map controls, and drag-and-drop support.

use crate::app::state::{AppState, SidebarTab, TilesProvider};
use egui::{Color32, RichText, Ui};

/// Render the sidebar toggle button (overlaid on top-right of map)
pub fn sidebar_toggle_button(ui: &mut Ui, state: &mut AppState) {
    let button_size = egui::vec2(40.0, 40.0);
    let margin = 10.0;

    // Position button in top-right corner
    let rect = ui.max_rect();
    let button_pos = rect.right_top() + egui::vec2(-button_size.x - margin, margin);
    let button_rect = egui::Rect::from_min_size(button_pos, button_size);

    let response = ui.allocate_rect(button_rect, egui::Sense::click());

    if response.clicked() {
        state.ui_settings.sidebar_open = !state.ui_settings.sidebar_open;
    }

    // Draw button background
    let bg_color = if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };

    ui.painter().rect_filled(
        button_rect,
        5.0, // rounding
        bg_color,
    );

    // Draw icon (hamburger menu always)
    let icon = if state.ui_settings.sidebar_open {
        "☰"
    } else {
        "☰"
    };

    ui.painter().text(
        button_rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(20.0),
        ui.visuals().text_color(),
    );
}

/// Helper to draw a control button
fn draw_control_button(ui: &mut Ui, rect: egui::Rect, icon: &str, tooltip: &str) -> bool {
    let response = ui.allocate_rect(rect, egui::Sense::click());

    let bg_color = if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };

    ui.painter().rect_filled(rect, 5.0, bg_color);

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(16.0),
        ui.visuals().text_color(),
    );

    let response = if response.hovered() {
        response.on_hover_text(tooltip)
    } else {
        response
    };

    response.clicked()
}

/// Render the main sidebar (responsive: side on landscape, bottom on portrait)
pub fn render_sidebar(ctx: &egui::Context, state: &mut AppState) {
    if !state.ui_settings.sidebar_open {
        return;
    }

    let screen_size = ctx.viewport_rect().size();
    let is_portrait = screen_size.y > screen_size.x;

    if is_portrait {
        render_sidebar_bottom(ctx, state);
    } else {
        render_sidebar_side(ctx, state);
    }
}

/// Render sidebar from the side (landscape mode)
fn render_sidebar_side(ctx: &egui::Context, state: &mut AppState) {
    egui::SidePanel::right("main_sidebar")
        .default_width(320.0)
        .min_width(280.0)
        .max_width(500.0)
        .resizable(true)
        .show(ctx, |ui| {
            render_sidebar_content(ui, state, false);
        });
}

/// Render sidebar from the bottom (portrait mode)
fn render_sidebar_bottom(ctx: &egui::Context, state: &mut AppState) {
    egui::TopBottomPanel::bottom("main_sidebar")
        .default_height(300.0)
        .min_height(200.0)
        .max_height(ctx.viewport_rect().height() * 0.6)
        .resizable(true)
        .show(ctx, |ui| {
            render_sidebar_content(ui, state, true);
        });
}

/// Render the sidebar content (shared between portrait and landscape)
fn render_sidebar_content(ui: &mut Ui, state: &mut AppState, is_portrait: bool) {
    // Tab selection
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut state.ui_settings.active_tab,
            SidebarTab::Tracks,
            "📂 Tracks",
        );
        ui.selectable_value(
            &mut state.ui_settings.active_tab,
            SidebarTab::Settings,
            "⚙ Settings",
        );
    });

    ui.separator();

    // Tab content
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.ui_settings.active_tab {
            SidebarTab::Tracks => render_tracks_tab(ui, state, is_portrait),
            SidebarTab::Settings => render_settings_tab(ui, state, is_portrait),
        });
}

/// Render the Tracks tab
fn render_tracks_tab(ui: &mut Ui, state: &mut AppState, is_portrait: bool) {
    ui.heading("Loaded Tracks");
    ui.add_space(4.0);

    // Action buttons
    if is_portrait {
        // Vertical layout for portrait
        ui.vertical(|ui| {
            if ui.button("📂 Load GPX Files...").clicked() {
                state.file_loader.show_picker = true;
            }
            if ui.button("🗑 Clear All").clicked() {
                state.clear_routes();
            }
        });
    } else {
        // Horizontal layout for landscape
        ui.horizontal(|ui| {
            if ui.button("📂 Load GPX Files...").clicked() {
                state.file_loader.show_picker = true;
            }
            if ui.button("🗑 Clear All").clicked() {
                state.clear_routes();
            }
        });
    }

    ui.add_space(8.0);

    // Loading progress
    if state.file_loader.is_busy() {
        ui.separator();
        ui.label(
            RichText::new("⏳ Loading...")
                .strong()
                .color(ui.visuals().warn_fg_color),
        );

        if let Some(ref loading) = state.file_loader.loading_file {
            ui.label(
                RichText::new(format!(
                    "Current: {}",
                    loading.file_name().unwrap_or_default().to_string_lossy()
                ))
                .small(),
            );
        }

        let total =
            state.file_loader.loaded_files.len() + state.file_loader.pending_files.len() + 1;
        let progress = state.file_loader.progress(total);
        ui.add(egui::ProgressBar::new(progress).show_percentage());

        ui.add_space(8.0);
    }

    ui.separator();

    // Statistics overview
    ui.label(RichText::new("📊 Overview").strong());
    ui.add_space(4.0);

    egui::Grid::new("stats_grid")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("Files:");
            ui.label(RichText::new(format!("{}", state.file_loader.loaded_files.len())).strong());
            ui.end_row();

            ui.label("Routes:");
            ui.label(RichText::new(state.stats.format_routes()).strong());
            ui.end_row();

            ui.label("Points:");
            ui.label(RichText::new(state.stats.format_points()).strong());
            ui.end_row();

            ui.label("Distance:");
            ui.label(RichText::new(state.stats.format_distance()).strong());
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.separator();

    // Loaded files list
    if !state.file_loader.loaded_files.is_empty() {
        ui.label(
            RichText::new("✓ Loaded Files")
                .strong()
                .color(Color32::GREEN),
        );
        ui.add_space(4.0);

        let mut to_remove = None;

        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for (idx, (path, _)) in state.file_loader.loaded_files.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "📄 {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ))
                            .small(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("🗑").clicked() {
                                to_remove = Some(idx);
                            }
                        });
                    });
                }
            });

        // Handle removal (can't modify while iterating)
        if let Some(idx) = to_remove {
            state.remove_file(idx);
        }

        ui.add_space(8.0);
    }

    // Error list
    if !state.file_loader.errors.is_empty() {
        ui.separator();
        ui.label(
            RichText::new(format!(
                "⚠ Errors ({} files)",
                state.file_loader.errors.len()
            ))
            .strong()
            .color(Color32::RED),
        );
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(150.0)
            .show(ui, |ui| {
                for (file, error) in &state.file_loader.errors {
                    ui.label(
                        RichText::new(format!(
                            "• {}: {}",
                            file.file_name().unwrap_or_default().to_string_lossy(),
                            error
                        ))
                        .small()
                        .color(Color32::RED),
                    );
                }
            });

        ui.add_space(4.0);
        if ui.button("Clear Errors").clicked() {
            state.file_loader.errors.clear();
        }
    }
}

/// Render the Settings tab
fn render_settings_tab(ui: &mut Ui, state: &mut AppState, _is_portrait: bool) {
    ui.heading("Settings");
    ui.add_space(4.0);

    // Track appearance
    ui.collapsing("Track Appearance", |ui| {
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Line Width:");
            ui.add(
                egui::Slider::new(&mut state.ui_settings.line_width, 0.5..=10.0)
                    .suffix(" px")
                    .step_by(0.5),
            );
        });

        ui.add_space(4.0);
    });

    ui.add_space(4.0);

    // Level of Detail
    ui.collapsing("Level of Detail", |ui| {
        ui.add_space(4.0);

        ui.label("LOD Bias (Higher = More Detail)");
        ui.add_space(4.0);

        let mut bias = state.ui_settings.bias;
        let changed = ui
            .add(
                egui::Slider::new(&mut bias, 0.1..=10.0)
                    .logarithmic(true)
                    .step_by(0.1),
            )
            .changed();

        if changed {
            state.update_bias(bias);
        }

        ui.add_space(4.0);
        ui.label(
            RichText::new("⚠ Note: Changing bias requires reloading routes")
                .small()
                .color(ui.visuals().warn_fg_color),
        );

        ui.add_space(4.0);
    });

    ui.add_space(4.0);

    // Map Tiles
    ui.collapsing("Map Tiles", |ui| {
        ui.add_space(4.0);

        ui.label("Select Tile Provider");
        ui.add_space(4.0);

        for provider in TilesProvider::all() {
            let selected = state.ui_settings.tiles_provider == *provider;
            if ui.selectable_label(selected, provider.name()).clicked() {
                state.ui_settings.tiles_provider = *provider;
            }
        }

        ui.add_space(4.0);
        ui.label(
            RichText::new(state.ui_settings.tiles_provider.attribution())
                .small()
                .italics(),
        );

        ui.add_space(4.0);
    });

    ui.add_space(4.0);

    // Debug options
    ui.collapsing("Debug", |ui| {
        ui.add_space(4.0);

        ui.checkbox(
            &mut state.ui_settings.show_boundary_debug,
            "Show boundary context markers",
        );
        ui.label(RichText::new("Green = prev point, Red = next point").small());

        ui.add_space(4.0);
    });

    ui.add_space(4.0);

    // Profiling
    ui.collapsing("Profiling", |ui| {
        ui.add_space(4.0);

        ui.checkbox(&mut state.ui_settings.show_profiling, "Show profiling data");

        ui.add_space(8.0);

        if state.ui_settings.show_profiling {
            ui.separator();
            egui_eframe_entrypoints::profiling_ui(ui);
        }

        ui.add_space(4.0);
    });

    ui.add_space(4.0);

    // Performance stats
    ui.collapsing("Performance", |ui| {
        ui.add_space(4.0);

        if state.stats.last_query_time_ms > 0.0 {
            egui::Grid::new("perf_grid")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Last Query:");
                    let color = if state.stats.last_query_time_ms < 16.0 {
                        Color32::GREEN
                    } else if state.stats.last_query_time_ms < 100.0 {
                        Color32::YELLOW
                    } else {
                        Color32::RED
                    };
                    ui.label(
                        RichText::new(format!("{:.1} ms", state.stats.last_query_time_ms))
                            .color(color),
                    );
                    ui.end_row();

                    ui.label("Segments Rendered:");
                    ui.label(
                        RichText::new(format!("{}", state.stats.last_query_segments)).strong(),
                    );
                    ui.end_row();
                });
        } else {
            ui.label(RichText::new("No query data yet").italics().weak());
        }

        ui.add_space(4.0);
    });
}

/// Render a simple file picker (native only)
#[cfg(not(target_arch = "wasm32"))]
pub fn show_file_picker(state: &mut AppState) {
    if state.file_loader.show_picker {
        state.file_loader.show_picker = false;

        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("GPX Files", &["gpx"])
            .add_filter("All Files", &["*"])
            .pick_files()
        // Changed to pick_files() to support multiple selection
        {
            for path in paths {
                state.queue_file(path);
            }
        }
    }
}

/// Render a simple file picker (web version - placeholder)
#[cfg(target_arch = "wasm32")]
pub fn show_file_picker(state: &mut AppState) {
    if state.file_loader.show_picker {
        state.file_loader.show_picker = false;
        // Web file picker would require async file reading
        // This is a placeholder for web implementation
        tracing::warn!("File picker not yet implemented for web");
    }
}

/// Render the help overlay
pub fn help_overlay(ctx: &egui::Context, show: &mut bool) {
    egui::Window::new("Help")
        .open(show)
        .collapsible(false)
        .resizable(true)
        .default_width(400.0)
        .show(ctx, |ui| {
            ui.heading("Large Track Viewer");
            ui.separator();

            ui.label("A high-performance viewer for large GPS track collections.");
            ui.add_space(8.0);

            ui.label(RichText::new("🖱 Map Controls").strong());
            ui.label("• Left drag: Pan the map");
            ui.label("• Mouse wheel: Zoom in/out");
            ui.label("• Double click: Zoom in");
            ui.label("• +/- buttons: Zoom (accessibility)");
            ui.add_space(8.0);

            ui.label(RichText::new("📂 Loading Tracks").strong());
            ui.label("• Click 'Load GPX Files' in sidebar");
            ui.label("• Drag & drop GPX files onto map");
            ui.label("• Multiple files can be selected");
            ui.label("• Large files are indexed automatically");
            ui.add_space(8.0);

            ui.label(RichText::new("⚙️ Settings").strong());
            ui.label("• Access via Settings tab in sidebar");
            ui.label("• Adjust LOD bias for detail level");
            ui.label("• Change colors and line width");
            ui.label("• Select different map tile providers");
            ui.add_space(8.0);

            ui.label(RichText::new("💡 Tips").strong());
            ui.label("• Toggle sidebar with ☰ button");
            ui.label("• Sidebar adapts to screen orientation");
            ui.label("• Press F1 to show/hide this help");
            ui.add_space(8.0);

            ui.separator();
            ui.label(
                RichText::new("Press F1 to toggle this help")
                    .small()
                    .italics(),
            );
        });
}

/// Handle drag and drop of GPX files
pub fn handle_drag_and_drop(ctx: &egui::Context, state: &mut AppState) {
    use egui::*;

    // Read input state first (without calling other ctx methods)
    let (has_hovered_files, dropped_files) =
        ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));

    // Preview dragged files - render OUTSIDE the input closure
    if has_hovered_files {
        let painter = ctx.layer_painter(LayerId::new(
            Order::Foreground,
            Id::new("drag_and_drop_overlay"),
        ));

        let screen_rect = ctx.viewport_rect();
        painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(180));

        painter.text(
            screen_rect.center(),
            Align2::CENTER_CENTER,
            "Drop GPX files here",
            FontId::proportional(32.0),
            Color32::WHITE,
        );
    }

    // Handle dropped files
    if !dropped_files.is_empty() {
        for file in &dropped_files {
            if let Some(path) = &file.path {
                // Check if it's a GPX file
                if path.extension().and_then(|s| s.to_str()) == Some("gpx") {
                    state.queue_file(path.clone());
                } else {
                    tracing::warn!("Ignoring non-GPX file: {:?}", path);
                }
            }
        }
    }
}

/// Show mouse wheel zoom warning overlay with smooth fade animation
pub fn show_wheel_zoom_warning(ui: &mut Ui, state: &mut AppState) {
    use egui::*;

    // Get fade alpha (0.0 to 1.0)
    let alpha = state.get_wheel_warning_alpha();

    if alpha <= 0.0 {
        return;
    }

    // Request repaint for animation
    ui.ctx().request_repaint();

    // Semi-transparent overlay at the bottom center
    let screen_rect = ui.max_rect();
    let warning_width = 400.0;
    let warning_height = 40.0;

    let warning_pos = Pos2::new(
        screen_rect.center().x - warning_width / 2.0,
        screen_rect.center().y - warning_height / 2.0,
    );

    let warning_rect = Rect::from_min_size(warning_pos, Vec2::new(warning_width, warning_height));

    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(warning_rect)
            .layout(Layout::top_down(Align::Center)),
    );

    // Background - transparent with fade
    let bg_alpha = (160.0 * alpha) as u8;
    let painter = ui.painter();
    painter.rect_filled(warning_rect, 8.0, Color32::from_black_alpha(bg_alpha));

    ui.add_space(4.0);

    // Icon and text with fade
    let text_alpha = (255.0 * alpha) as u8;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new("🖱  Use Ctrl + scroll to zoom the map")
                .size(24.0)
                .color(Color32::from_rgba_premultiplied(255, 255, 255, text_alpha)),
        );
    });
}

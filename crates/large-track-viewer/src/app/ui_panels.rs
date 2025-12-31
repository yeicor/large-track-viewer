//! UI panels for the application
//!
//! This module provides reusable UI components for the sidebar design
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

    // Draw icon (hamburger menu or X based on state)
    let icon = if state.ui_settings.sidebar_open {
        "✕"
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
        .default_width(300.0)
        .min_width(260.0)
        .max_width(450.0)
        .resizable(true)
        .show(ctx, |ui| {
            render_sidebar_content(ui, state, false);
        });
}

/// Render sidebar from the bottom (portrait mode)
fn render_sidebar_bottom(ctx: &egui::Context, state: &mut AppState) {
    egui::TopBottomPanel::bottom("main_sidebar")
        .default_height(280.0)
        .min_height(180.0)
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
            SidebarTab::Settings => render_settings_tab(ui, state),
        });
}

/// Render the Tracks tab
fn render_tracks_tab(ui: &mut Ui, state: &mut AppState, is_portrait: bool) {
    // Action buttons at top
    if is_portrait {
        ui.vertical(|ui| {
            if ui.button("📂 Load GPX Files...").clicked() {
                state.file_loader.show_picker = true;
            }
            ui.horizontal(|ui| {
                if ui.button("🎯 Fit to Bounds").clicked() {
                    state.pending_fit_bounds = true;
                }
                if ui.button("🗑 Clear All").clicked() {
                    state.clear_routes();
                }
            });
        });
    } else {
        ui.horizontal(|ui| {
            if ui.button("📂 Load GPX Files...").clicked() {
                state.file_loader.show_picker = true;
            }
            if ui.button("🎯 Fit to Bounds").clicked() {
                state.pending_fit_bounds = true;
            }
            if ui.button("🗑 Clear All").clicked() {
                state.clear_routes();
            }
        });
    }

    ui.add_space(8.0);

    // Loading progress
    if state.file_loader.is_busy() || state.is_parallel_loading() {
        ui.separator();

        let status = state.loading_status();
        ui.label(
            RichText::new(format!("⏳ Loading files... ({})", status))
                .strong()
                .color(ui.visuals().warn_fg_color),
        );
        let progress = state.loading_progress();
        ui.add(egui::ProgressBar::new(progress).show_percentage());

        ui.add_space(8.0);
    }

    ui.separator();

    // Statistics - always visible and prominent
    render_stats_section(ui, state);

    ui.add_space(8.0);
    ui.separator();

    // Error list (shown BEFORE loaded files, with fixed height)
    if !state.file_loader.errors.is_empty() {
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
            .id_salt("errors_scroll")
            .max_height(100.0)
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

        ui.add_space(8.0);
        ui.separator();
    }

    // Loaded files list (expands to fill remaining available space)
    if !state.file_loader.loaded_files.is_empty() {
        ui.label(
            RichText::new("✓ Loaded Files")
                .strong()
                .color(Color32::GREEN),
        );
        ui.add_space(4.0);

        let mut to_remove = None;

        // Use all remaining available height for the loaded files list
        let available_height = ui.available_height().max(80.0);

        egui::ScrollArea::vertical()
            .id_salt("loaded_files_scroll")
            .max_height(available_height - 8.0) // Leave small margin at bottom
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

        if let Some(idx) = to_remove {
            state.remove_file(idx);
        }
    }
}

/// Render statistics section (used in Tracks tab)
fn render_stats_section(ui: &mut Ui, state: &AppState) {
    ui.label(RichText::new("📊 Statistics").strong());
    ui.add_space(4.0);

    egui::Grid::new("stats_grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            // Data stats
            ui.label("Files:");
            ui.label(RichText::new(format!("{}", state.file_loader.loaded_files.len())).strong());
            ui.end_row();

            ui.label("Routes:");
            ui.label(RichText::new(state.stats.format_routes()).strong());
            ui.end_row();

            ui.label("Total Points:");
            ui.label(RichText::new(state.stats.format_points()).strong());
            ui.end_row();

            ui.label("Distance:");
            ui.label(RichText::new(state.stats.format_distance()).strong());
            ui.end_row();

            // Performance stats (if we have query data)
            if state.stats.last_query_time_ms > 0.0 {
                ui.separator();
                ui.separator();
                ui.end_row();

                ui.label("Query Time:");
                let time_color = if state.stats.last_query_time_ms < 16.0 {
                    Color32::GREEN
                } else if state.stats.last_query_time_ms < 50.0 {
                    Color32::YELLOW
                } else {
                    Color32::RED
                };
                ui.label(
                    RichText::new(format!("{:.1} ms", state.stats.last_query_time_ms))
                        .color(time_color),
                );
                ui.end_row();

                ui.label("Segments:");
                ui.label(RichText::new(format!("{}", state.stats.last_query_segments)).strong());
                ui.end_row();

                ui.label("Points Rendered:");
                let reduction_text = if state.stats.total_points > 0 {
                    let pct = 100.0
                        * (1.0
                            - state.stats.last_query_simplified_points as f64
                                / state.stats.total_points as f64);
                    format!(
                        "{} ({:.0}% reduced)",
                        state.stats.last_query_simplified_points, pct
                    )
                } else {
                    format!("{}", state.stats.last_query_simplified_points)
                };
                ui.label(RichText::new(reduction_text).strong());
                ui.end_row();
            }
        });
}

/// Render the Settings tab
fn render_settings_tab(ui: &mut Ui, state: &mut AppState) {
    // Track Appearance section
    ui.label(RichText::new("🎨 Track Appearance").strong());
    ui.add_space(6.0);

    egui::Grid::new("appearance_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Line Width:");
            ui.add(
                egui::Slider::new(&mut state.ui_settings.line_width, 0.5..=8.0)
                    .suffix(" px")
                    .step_by(0.5),
            );
            ui.end_row();

            ui.label("Show Outline:");
            ui.checkbox(
                &mut state.ui_settings.show_outline,
                "Dark border for visibility",
            );
            ui.end_row();
        });

    ui.add_space(4.0);
    ui.label(
        RichText::new("Each route is automatically assigned a unique color")
            .small()
            .weak(),
    );

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);

    // Level of Detail section
    ui.label(RichText::new("📐 Level of Detail").strong());
    ui.add_space(6.0);

    ui.label("LOD Bias (Higher = More Detail):");
    ui.add_space(4.0);

    let mut bias = state.ui_settings.bias;
    let bias_changed = ui
        .add(
            egui::Slider::new(&mut bias, 0.001..=1000.0)
                .logarithmic(true)
                .custom_formatter(|v, _| {
                    if v >= 1.0 {
                        format!("{:.0}", v)
                    } else if v >= 0.01 {
                        format!("{:.2}", v)
                    } else {
                        format!("{:.3}", v)
                    }
                }),
        )
        .changed();

    if bias_changed {
        state.update_bias(bias);
    }

    if state.pending_reload && !state.file_loader.loaded_files.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new("⏳ Reloading with new LOD settings...")
                .small()
                .color(ui.visuals().warn_fg_color),
        );
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);

    // Map Tiles section
    ui.label(RichText::new("🗺 Map Tiles").strong());
    ui.add_space(6.0);

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
            .italics()
            .weak(),
    );

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);

    ui.add_space(4.0);

    // Debug section
    ui.label(RichText::new("🔧 Debug").strong());
    ui.add_space(6.0);

    ui.checkbox(&mut state.ui_settings.show_profiling, "Show profiling data");
    if state.ui_settings.show_profiling {
        ui.add_space(4.0);
        eframe_entrypoints::profiling_ui(ui);
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);

    // About section
    ui.label(RichText::new("ℹ About").strong());
    ui.add_space(4.0);
    ui.label(RichText::new("Large Track Viewer").small());
    ui.label(
        RichText::new("Efficiently view large GPS tracks with LOD")
            .small()
            .weak(),
    );
    ui.add_space(4.0);
    ui.label(RichText::new("Keyboard shortcuts:").small());
    ui.label(RichText::new("  F1 / Ctrl+H - Toggle help").small().weak());
    ui.label(RichText::new("  Ctrl + Scroll - Zoom map").small().weak());
}

/// Show file picker dialog
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn show_file_picker(state: &mut AppState) {
    if state.file_loader.show_picker {
        state.file_loader.show_picker = false;

        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("GPX Files", &["gpx"])
            .set_title("Select GPX Files")
            .pick_files()
        {
            for path in paths {
                state.queue_file(egui::DroppedFile {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_str()
                        .unwrap_or_default()
                        .to_owned(),
                    path: Some(path),
                    ..Default::default()
                });
            }
            // Start parallel loading for newly added files
            state.start_parallel_load();
        }
    }
}

/// Help overlay
pub fn help_overlay(ctx: &egui::Context, show_help: &mut bool) {
    egui::Window::new("Help")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.heading("Large Track Viewer");
            ui.add_space(8.0);

            ui.label("A fast viewer for large GPS tracks with automatic level-of-detail.");
            ui.add_space(12.0);

            ui.label(RichText::new("Loading Tracks").strong());
            ui.label("• Click 'Load GPX Files...' in the sidebar");
            ui.label("• Or drag and drop GPX files onto the window");
            ui.add_space(8.0);

            ui.label(RichText::new("Navigation").strong());
            ui.label("• Ctrl + Scroll wheel to zoom");
            ui.label("• Click and drag to pan");
            ui.label("• 'Fit to Bounds' to see all tracks");
            ui.add_space(8.0);

            ui.label(RichText::new("Keyboard Shortcuts").strong());
            ui.label("• F1 or Ctrl+H - Toggle this help");
            ui.add_space(12.0);

            if ui.button("Close").clicked() {
                *show_help = false;
            }
        });
}

/// Handle drag and drop of GPX files
pub fn handle_drag_and_drop(ctx: &egui::Context, state: &mut AppState) {
    // Only read input state inside ctx.input
    let hovered_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
    let dropped_files: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());

    // Show drop preview if files are hovered
    if hovered_files {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop_preview"),
        ));
        let screen_rect = ctx.content_rect();
        let bg_size = egui::vec2(340.0, 80.0);
        let bg_rect = egui::Rect::from_center_size(screen_rect.center(), bg_size);
        painter.rect_filled(
            bg_rect,
            16.0, // rounding
            egui::Color32::from_black_alpha(180),
        );
        painter.text(
            screen_rect.center(),
            egui::Align2::CENTER_CENTER,
            "📂 Drop GPX files here",
            egui::FontId::proportional(32.0),
            egui::Color32::WHITE,
        );
    }

    // Handle dropped files outside of ctx.input
    let mut files_dropped = false;
    for dropped_file in dropped_files {
        if dropped_file
            .path
            .as_ref()
            .unwrap()
            .extension()
            .map(|e| e == "gpx")
            .unwrap_or(false)
        {
            state.queue_file(dropped_file.clone());
            files_dropped = true;
        }
    }
    if files_dropped {
        state.start_parallel_load();
    }
}

/// Show mouse wheel zoom warning
pub fn show_wheel_zoom_warning(ui: &mut Ui, state: &mut AppState) {
    let alpha = state.get_wheel_warning_alpha();
    if alpha <= 0.0 {
        return;
    }

    let rect = ui.max_rect();
    let warning_size = egui::vec2(280.0, 50.0);
    let warning_pos = rect.center() - warning_size / 2.0;
    let warning_rect = egui::Rect::from_min_size(warning_pos, warning_size);

    // Background with fade
    let bg_alpha = (180.0 * alpha) as u8;
    ui.painter().rect_filled(
        warning_rect,
        10.0,
        egui::Color32::from_black_alpha(bg_alpha),
    );

    // Text with fade
    let text_alpha = (255.0 * alpha) as u8;
    ui.painter().text(
        warning_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Hold Ctrl + Scroll to zoom",
        egui::FontId::proportional(16.0),
        egui::Color32::from_white_alpha(text_alpha),
    );
}

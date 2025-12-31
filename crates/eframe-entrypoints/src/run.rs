//! Generic application runner for egui/eframe applications
//!
//! This module provides generic entry point functions that can be used by any
//! egui/eframe application. For the recommended API, use the `eframe_app!` macro.

/// Setup function for native entry point
///
/// This is a simple wrapper that just returns the app creator.
/// Applications can override this to add custom initialization logic.
#[allow(dead_code)]
pub async fn setup_app<F>(app_creator: F) -> Option<F>
where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
{
    Some(app_creator)
}

/// Native entry point - generic version
///
/// This function can be called by any application to start an eframe app on native platforms.
/// For a cleaner API, consider using the `eframe_app!` macro instead.
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
pub async fn native_main_generic<F>(app_name: &str, app_creator: F)
where
    F: FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App>,
{
    crate::native_main_impl(app_name, app_creator).await;
}

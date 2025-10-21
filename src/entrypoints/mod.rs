// Shared modules
pub(crate) mod cli;
mod metadata;
mod run;

#[cfg(target_arch = "wasm32")]
mod web;

// Entry points
pub mod lib;
pub mod main;

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

eframe_entrypoints::eframe_app_main!();

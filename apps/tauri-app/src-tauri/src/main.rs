// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Fix for EGL_BAD_PARAMETER error on some Linux systems (white screen)
    // std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    steamos_mount_tauri_lib::run()
}

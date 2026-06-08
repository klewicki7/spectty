// Prevent a second console window on Windows in release builds. Harmless on the
// macOS target Spectty ships first.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    spectty_lib::run();
}

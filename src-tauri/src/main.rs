// CloudSaw — Tauri 2 entry point.
//
// The frontend is untrusted UI. All security decisions, validation, and
// privileged actions happen here, in Rust. See CLAUDE.md §4.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cloudsaw_lib::run();
}

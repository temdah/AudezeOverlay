#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod platform;
#[cfg(any(windows, target_os = "linux"))]
mod ui;

#[cfg(any(windows, target_os = "linux"))]
fn main() {
    ui::run();
}

/// The overlay window exists for Windows and Linux (Wayland).
#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("The overlay window is not available on this platform.");
    std::process::exit(2);
}

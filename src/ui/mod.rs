//! Native overlay presentation and event handling.
mod app;
mod draw;
mod number_roll;
mod presentation;
mod shell;
mod view;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(windows)]
mod win32;
#[cfg(target_os = "linux")]
pub use wayland::main as run;
#[cfg(windows)]
pub use win32::main as run;

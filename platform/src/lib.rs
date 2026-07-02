//! Platform-specific bits: RuneLite install discovery, per-OS quirks.
//! v1 targets Windows only — `linux` is added when Linux support is picked up post-v1.

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "windows")]
pub use windows::find_runelite;

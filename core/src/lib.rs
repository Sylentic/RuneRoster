//! Platform-agnostic core logic for RuneRoster: Jagex OAuth, multi-account profiles,
//! character listing, and spawning RuneLite. No UI dependencies live here.

pub mod accounts;
pub mod auth;
pub mod characters;
pub mod launcher;
pub mod runelite_config;
pub mod session;
pub mod store;

pub use accounts::Profile;

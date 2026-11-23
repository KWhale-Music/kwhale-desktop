#![allow(clippy::collapsible_if)]

pub mod cache_proxy;
pub mod engine;
pub mod mpris;

pub use engine::{measure_duration_sec, AudioEngine};
pub use mpris::{spawn_mpris, spawn_single_instance_dbus};

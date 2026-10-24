//! findamodel backend: indexes a folder of 3D models, generates previews and footprint
//! hulls, and serves the catalog, printing lists and plate exports over HTTP.

pub mod app;
pub mod config;
pub mod db;
pub mod dto;
pub mod entities;
pub mod error;
pub mod geometry;
pub mod logging;
pub mod metadata;
pub mod preview;
pub mod routes;
pub mod services;

#[cfg(test)]
pub mod test_support;

pub use app::{ServerHandle, run, start};
pub use config::Settings;
pub use logging::LogBuffer;

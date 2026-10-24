//! Application services. Each service owns a slice of behaviour and is usable without
//! the HTTP layer (handlers in `routes` are thin wrappers).

pub mod app_config;
pub mod explorer;
pub mod indexer;
pub mod jobs;
pub mod metadata_config;
pub mod metadata_dictionary;
pub mod models;
pub mod plate_export;
pub mod printers;
pub mod printing_lists;
pub mod query;
pub mod scan;
pub mod scan_config;
pub mod stats;
pub mod store;
pub mod users;

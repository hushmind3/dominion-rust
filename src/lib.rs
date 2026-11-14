//! DOMINION's native Rust engine and first-party in-process agent SDK.
//! The CLI and Rust library share the exact same runtime, tools and data store.
mod api;
pub mod bootstrap;
mod config;
mod db;
mod engine;
mod events;
mod migrations;
mod platform;
pub mod sdk;
mod state;
mod vault;

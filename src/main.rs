// Enforce zero production compiler warnings: dead code, stale imports and
// unused bindings must be removed or made explicitly test-only.
// Tests additionally reject unused imports/locals; test fixtures may hold
// unused helpers intentionally for cross-module regression coverage.
#![cfg_attr(not(test), deny(warnings))]
#![cfg_attr(test, deny(unused_imports, unused_variables))]

mod api;
mod bootstrap;
mod config;
mod db;
mod engine;
mod events;
mod migrations;
mod platform;
mod state;
mod vault;

#[tokio::main]
async fn main() {
    if let Err(error) = bootstrap::run(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

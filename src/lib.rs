//! Dominion native Rust SDK and owned execution runtime.
pub mod api;
pub mod config;
pub mod context;
pub mod domain;
pub mod engine;
pub mod group;
pub mod improvement;
pub mod mcp;
pub mod provider;
pub mod sdk;
pub mod store;
pub mod tools;
pub mod video;
pub mod veo;
pub use sdk::Dominion;
pub use config::{ModelSettings,Settings};
pub use domain::{CreateRun,Run,RunStatus,TimelineEvent,ToolCall,ReconcileTool,Approval};

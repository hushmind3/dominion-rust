//! Dominion native Rust SDK and owned execution runtime.
pub mod addons;
pub mod product;
// Public compatibility names point at the application layer.
pub use product::{group,knowledge,profile,provider_registry,frontend_state};
pub use addons::{channels as frontend_channels, connected_tools,
    extensions as frontend_extensions, github_cli_login, github_work,
    google_workspace, mcp, oauth as frontend_oauth,
    skill_import as frontend_skill_import};
pub mod api;
pub mod build_identity;
pub mod config;
pub mod context;
pub mod domain;
pub mod engine;
pub mod github_collab;
pub mod journal;
pub mod loop_guard;
pub mod hooks;
pub mod tool_schema;
pub mod tool_discovery;
pub mod tool_batch;
pub mod read_program;
pub mod program_ir;
pub mod program_session;
pub mod strategy;
pub mod policy;
pub mod provider;
pub mod frontend_api;
pub mod frontend_rooms;
pub mod frontend_providers;
pub mod frontend_codex_login;
pub mod frontend_provider_public;
pub(crate) mod frontend_public_http;
pub mod frontend_growth;
pub mod frontend_personas;
pub mod frontend_system;
pub mod frontend_local_models;
pub mod frontend_model_downloads;
pub mod frontend_native_picker;
pub mod frontend_huggingface;
pub mod sandbox;
pub mod sdk;
pub mod skills;
pub mod store;
pub mod tools;
pub mod verification;
pub mod acceptance;
pub mod repair_guidance;
pub mod workspace;
pub use sdk::Dominion;
pub use config::{ModelSettings,Settings};
pub use domain::{CreateRun,Run,RunStatus,TimelineEvent,ToolCall,ReconcileTool,Approval};

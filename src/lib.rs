//! Dominion native Rust SDK and owned execution runtime.
pub mod addons;
pub use addons::{github_collab,skills};
pub mod product;
pub use product::{native_api as api,build_identity};
pub use product::strategy;
// Compatible public paths point into product/; the native Run kernel does not
// own React API, room UI, provider pickers or platform configuration.
pub use product::{frontend_api,frontend_codex_login,frontend_growth,frontend_huggingface,frontend_local_models,frontend_model_downloads,frontend_native_picker,frontend_personas,frontend_provider_public,frontend_providers,frontend_rooms,frontend_system};
pub(crate) use product::frontend_public_http;
// Public compatibility names point at the application layer.
pub use product::{group,knowledge,profile,provider_registry,frontend_state};
pub use addons::{channels as frontend_channels, connected_tools,
    extensions as frontend_extensions, github_cli_login, github_work,
    google_workspace, mcp, oauth as frontend_oauth,
    skill_import as frontend_skill_import};

pub mod context;
pub mod domain;
pub mod engine;
pub mod journal;
pub mod loop_guard;
pub mod hooks;
pub mod tool_schema;
pub mod tool_discovery;
pub mod tool_batch;



pub mod policy;


pub mod sdk;
pub mod infrastructure;
pub use infrastructure::{model_transport as provider, local_tools as tools, sandbox, workspace};
pub use addons::{read_program,program_ir,program_session};
pub use product::config;
pub mod ports;
// Public compatibility alias, actual DB implementation is outside the core.
pub use infrastructure::persistence as store;

pub mod verification;
pub mod acceptance;
pub mod repair_guidance;

pub use sdk::Dominion;
pub use config::{ModelSettings,Settings};
pub use domain::{CreateRun,Run,RunStatus,TimelineEvent,ToolCall,ReconcileTool,Approval};

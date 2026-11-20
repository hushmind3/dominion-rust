//! Application composition root: actual file storage and external adapters
//! are chosen here, not by the natural-language instruction executor.
use crate::{algorithms::{AgentAlgorithm,DefaultAlgorithm},runtime_services::RuntimeServices,config::Settings,engine::Engine,group::Groups,mcp::Mcp,
    ports::{RunRepository,RunStorePort},store::Store};
use std::sync::Arc;

pub async fn open_run_repository(settings:&Settings)->Result<Arc<Store>,String>{
    Ok(Arc::new(Store::open_sqlite(settings.data_dir.join("runs")).await?))
}
impl Engine{
    pub fn new(settings:Settings,store:Arc<Store>,groups:Arc<Groups>)
        ->Result<Arc<Self>,String>{
        let mcp=Arc::new(if settings.addons_enabled{
            Mcp::from_env()?
        }else{
            Mcp::new(Vec::new())?
        });
        Self::new_with_mcp(settings,store,groups,mcp)
    }
    /// Compatibility adapter for existing tests and callers. Backend
    /// injection uses the same port as the production JSON storage.
    pub fn new_with_mcp(settings:Settings,store:Arc<Store>,
        groups:Arc<Groups>,mcp:Arc<Mcp>)->Result<Arc<Self>,String>{
        let backend:Arc<dyn RunRepository>=store;
        Self::new_with_repository(settings,backend,groups,mcp)
    }
    /// Core accepts any RunRepository. Storage drivers, file formats and
    /// durable storage location cannot be selected by a model or Run.
    pub fn new_with_repository(settings:Settings,backend:Arc<dyn RunRepository>,
        groups:Arc<Groups>,mcp:Arc<Mcp>)->Result<Arc<Self>,String>{
        Self::new_with_algorithm(settings,backend,groups,mcp,owner_algorithm()?)
    }
    /// Swap planning/context/tool-presentation logic without modifying or
    /// rebuilding the Run kernel. Only the host can supply an algorithm.
    pub fn new_with_algorithm(settings:Settings,backend:Arc<dyn RunRepository>,
        groups:Arc<Groups>,mcp:Arc<Mcp>,algorithm:Arc<dyn AgentAlgorithm>)
        ->Result<Arc<Self>,String>{
        Self::new_with_algorithm_and_loop(settings,backend,groups,mcp,
            algorithm,Arc::new(dominion_agent_algo::ReActLoop))
    }
    /// Inject a custom pure loop policy into the runtime. Its decisions
    /// still pass the kernel's mandatory authorization/state gate.
    pub fn new_with_algorithm_and_loop(settings:Settings,
        backend:Arc<dyn RunRepository>,groups:Arc<Groups>,mcp:Arc<Mcp>,
        algorithm:Arc<dyn AgentAlgorithm>,
        loop_driver:Arc<dyn dominion_agent_core::AgentLoop>)
        ->Result<Arc<Self>,String>{
        let product=super::ProductHost::open(&settings,groups,mcp)?;
        Engine::from_ports_with_loop(settings,
            Arc::new(RunStorePort::new(backend)),product,algorithm,loop_driver)
    }
    /// Full host injection for embedders. The supplied port may choose any
    /// model, memory backend and external tool adapter, but cannot replace
    /// the native intent journal, approval gate or receipt admission logic.
    pub fn new_with_runtime_services(settings:Settings,
        backend:Arc<dyn RunRepository>,groups:Arc<Groups>,mcp:Arc<Mcp>,
        algorithm:Arc<dyn AgentAlgorithm>,
        loop_driver:Arc<dyn dominion_agent_core::AgentLoop>,
        services:Arc<dyn RuntimeServices>)->Result<Arc<Self>,String>{
        // Compatibility product services remain available to HTTP/SDK callers
        // while the native execution kernel exclusively uses the injected port.
        let product=Arc::new(super::ProductHost::open(&settings,groups,mcp)?);
        Engine::from_ports_with_services(settings,
            Arc::new(RunStorePort::new(backend)),product,algorithm,
            loop_driver,services)
    }
}
/// Operator-owned optional prompt profile. No Run can read arbitrary files
/// or change its algorithm; a new start pins the strategy identity.
fn owner_algorithm()->Result<Arc<dyn AgentAlgorithm>,String>{
    let path=match std::env::var("DOMINION_AGENT_PROMPT_FILE"){
        Ok(v) if !v.trim().is_empty()=>v,
        Ok(_)|Err(std::env::VarError::NotPresent)=>{
            return Ok(Arc::new(DefaultAlgorithm::default()));
        },
        Err(e)=>return Err(format!("invalid algorithm prompt setting: {e}")),
    };
    let metadata=std::fs::metadata(&path)
        .map_err(|e|format!("cannot read configured agent prompt {path}: {e}"))?;
    if !metadata.is_file()||metadata.len()>64*1024{
        return Err("agent prompt file must be a regular UTF-8 file <=64 KiB".into());
    }
    let text=std::fs::read_to_string(&path)
        .map_err(|e|format!("configured agent prompt is unreadable: {e}"))?;
    Ok(Arc::new(DefaultAlgorithm::with_prompt(text)?))
}

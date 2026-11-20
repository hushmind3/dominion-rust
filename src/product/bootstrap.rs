//! Application composition root: actual file storage and external adapters
//! are chosen here, not by the natural-language instruction executor.
use crate::{config::Settings,engine::Engine,group::Groups,mcp::Mcp,
    ports::{RunRepository,RunStorePort},store::Store};
use std::sync::Arc;

pub async fn open_run_repository(settings:&Settings)->Result<Arc<Store>,String>{
    Ok(Arc::new(Store::open(settings.data_dir.join("runs")).await?))
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
        let product=super::ProductHost::open(&settings,groups,mcp)?;
        Engine::from_ports(settings,Arc::new(RunStorePort::new(backend)),product)
    }
}

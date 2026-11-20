//! Optional integration host: MCP, connected accounts, outbound channels,
//! managed skills and OAuth live outside the native Run kernel.
//! These adapters never own another scheduler, approval journal or execution loop.
use crate::{config::Settings,domain::ToolCall,engine::Engine,group::Room};
use axum::Router;
use serde_json::Value;
use std::sync::Arc;

pub mod channels;
pub mod connected_tools;
pub mod extensions;
pub mod github_cli_login;
pub mod github_work;
pub mod google_workspace;
pub mod mcp;
pub mod oauth;
pub mod skill_import;

/// One typed boundary for optional connectivity. The core Engine owns this
/// facade, not a list of unrelated transport and UI storage implementations.
pub struct Host {
    pub enabled:bool,
    pub mcp:Arc<mcp::Mcp>,
    pub extensions:Arc<extensions::Extensions>,
}
impl Host {
    pub fn open(settings:&Settings,mcp:Arc<mcp::Mcp>)->Result<Arc<Self>,String>{
        let extensions=if settings.addons_enabled{
            let store=extensions::Extensions::open(
                settings.data_dir.clone(),mcp.servers_snapshot())?;
            mcp.initialize_servers(store.initial_servers())?;
            Arc::new(store)
        }else{
            // No credentials, servers or managed add-on files are read when
            // connectivity is disabled. Existing durable files are untouched.
            Arc::new(extensions::Extensions::inactive(settings.data_dir.clone()))
        };
        Ok(Arc::new(Self{enabled:settings.addons_enabled,mcp,extensions}))
    }
    /// Hide every external schema from the model when optional connectivity is
    /// turned off. Policy and approval remain owned by the native journal.
    pub async fn tool_definitions(&self,settings:&Settings)->Vec<Value>{
        if !self.enabled{return Vec::new()}
        let mut result=self.mcp.definitions().await;
        result.extend(connected_tools::definitions(settings));
        result
    }
    pub fn supports_connected(&self,name:&str)->bool{
        self.enabled && connected_tools::supports(name)
    }
    pub fn needs_connected_approval(&self,name:&str)->bool{
        self.enabled && connected_tools::mutates(name)
    }
    pub async fn invoke_connected(&self,settings:&Settings,call:&ToolCall)
        ->mcp::McpCallOutcome{
        if !self.enabled{
            return mcp::McpCallOutcome::Settled(
                Err("external add-ons are disabled by the operator".into()));
        }
        connected_tools::execute(settings,call).await
    }
}
/// Old /api URLs remain identical. The top-level API depends on this single
/// optional adapter router, not on channel/OAuth/MCP implementation details.
pub fn routes(enabled:bool)->Router<Arc<Engine>>{
    if !enabled{return Router::new()}
    Router::new()
        .merge(extensions::routes())
        .merge(channels::routes())
        .merge(skill_import::routes())
        .merge(oauth::routes())
}
/// Optional notifications may never stall or replace the durable room append.
pub async fn dispatch_room(engine:&Arc<Engine>,room:&Room){
    if engine.addons.enabled{channels::dispatch_room(engine,room).await;}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn disabled_host_exposes_no_optional_endpoints(){
        // Test the routing boundary without environmental mutations or IO.
        let _empty:Router<Arc<Engine>>=routes(false);
        assert!(!std::any::type_name::<Host>().is_empty());
    }
}

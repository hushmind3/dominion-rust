//! Optional integration host: MCP, connected accounts, outbound channels,
//! managed skills and OAuth live outside the native Run kernel.
//! These adapters never own another scheduler, approval journal or execution loop.
use crate::{config::Settings,domain::ToolCall,engine::Engine,group::Room};
use axum::Router;
use serde_json::Value;
use std::sync::Arc;

pub mod read_program;
pub mod program_ir;
pub mod program_session;
pub mod channels;
pub mod agent_bridge;
pub mod execution;
pub mod connected_tools;
pub mod extensions;
pub mod github_cli_login;
pub mod github_work;
pub mod github_collab;
pub mod skills;
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
    pub bridge:agent_bridge::BridgeRegistry,
}
impl Host {
    pub fn open(settings:&Settings,mcp:Arc<mcp::Mcp>)->Result<Arc<Self>,String>{
        let extensions=if settings.addons_enabled{
            let store=extensions::Extensions::open(
                settings.data_dir.clone(),mcp.servers_snapshot())?;
            mcp.initialize_servers(store.initial_servers())?;
            Arc::new(store)
        }else{
            // Even an injected/reused MCP handle must lose its server list:
            // disabled connectivity cannot expose stale remote metadata.
            mcp.initialize_servers(Vec::new())?;
            // No credentials, servers or managed add-on files are read when
            // connectivity is disabled. Existing durable files are untouched.
            Arc::new(extensions::Extensions::inactive(settings.data_dir.clone()))
        };
        let bridge=if settings.addons_enabled{
            agent_bridge::BridgeRegistry::from_env()?
        }else{agent_bridge::BridgeRegistry::disabled()};
        Ok(Arc::new(Self{enabled:settings.addons_enabled,mcp,extensions,bridge}))
    }
    /// Hide every external schema from the model when optional connectivity is
    /// turned off. Policy and approval remain owned by the native journal.
    pub async fn tool_definitions(&self,settings:&Settings)->Vec<Value>{
        if !self.enabled{return Vec::new()}
        let mut result=self.mcp.definitions().await;
        result.extend(connected_tools::definitions(settings));
        result.extend(self.bridge.definitions());
        result
    }
    /// The Run kernel sees one external-tool port, not MCP/connected-tool
    /// transports. A disabled or revoked remote endpoint never falls back
    /// to an unapproved native tool call.
    pub fn handles_tool(&self,name:&str)->bool{
        matches!(name,agent_bridge::TOOL|agent_bridge::EDIT_TOOL) || name.starts_with("mcp__") || connected_tools::supports(name)
    }
    pub fn needs_external_approval(&self,name:&str)->bool{
        matches!(name,agent_bridge::TOOL|agent_bridge::EDIT_TOOL) || name.starts_with("mcp__") || (self.enabled && connected_tools::mutates(name))
    }
    pub fn needs_connected_approval(&self,name:&str)->bool{
        matches!(name,agent_bridge::TOOL|agent_bridge::EDIT_TOOL) || (self.enabled && connected_tools::mutates(name))
    }
    pub async fn invoke_external(
        &self,settings:&Settings,store:&Arc<crate::ports::RunStorePort>,
        id:uuid::Uuid,call:&ToolCall,cancel:tokio::sync::watch::Receiver<bool>
    )->Result<crate::domain::ToolEffectOutcome,String>{
        execution::invoke(self,settings,store,id,call,cancel).await
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
    use crate::{group::Groups,store::Store};
    use uuid::Uuid;

    #[tokio::test]
    async fn disabled_addons_leave_native_http_alive_without_loading_external_secrets(){
        let root=std::env::temp_dir().join(format!("dominion-addons-off-{}",Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        // A corrupt old optional config must not prevent ordinary native Runs.
        tokio::fs::write(root.join("extensions.json"),"{not-valid-json").await.unwrap();
        let mut settings=Settings::from_env().unwrap();
        settings.addons_enabled=false;
        settings.token=None;
        settings.data_dir=root.clone();
        settings.workspace_dir=root.join("workspace");
        let injected=Arc::new(mcp::Mcp::new(vec![mcp::Server{
            name:"test-server".into(),url:"http://127.0.0.1:9/mcp".into(),
            bearer_token:"do-not-use".into(),
        }]).unwrap());
        let host=Host::open(&settings,Arc::clone(&injected)).unwrap();
        assert_eq!(injected.server_count(),0,"disabled host must drop stale remote endpoints");
        assert!(host.tool_definitions(&settings).await.is_empty());
        assert!(host.extensions.skill_list().await.is_empty());
        assert!(host.handles_tool("mcp__test-server__search"));
        assert!(host.needs_external_approval("mcp__test-server__search"));

        let store=Arc::new(Store::open(root.join("runs")).await.unwrap());
        // A previously advertised MCP tool cannot escape through the
        // builtin executor after its extension host is disabled.
        let call=ToolCall{id:"blocked-remote".into(),
            name:"mcp__test-server__search".into(),arguments:serde_json::json!({})};
        let (_sender,receiver)=tokio::sync::watch::channel(false);
        let port=Arc::new(crate::ports::RunStorePort::new(store.clone()));
        let outcome=host.invoke_external(
            &settings,&port,Uuid::new_v4(),&call,receiver).await.unwrap();
        assert!(matches!(outcome,crate::domain::ToolEffectOutcome::Settled(Err(_))),
            "disabled external requests must not reach remote IO");
        let groups=Arc::new(Groups::open(root.join("groups")).await.unwrap());
        let engine=Engine::new(settings,store,groups).unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let router=crate::api::router(Arc::clone(&engine));
        let server=tokio::spawn(async move{axum::serve(listener,router).await.unwrap();});
        let client=reqwest::Client::new();
        for path in ["/api/health","/api/status"]{
            let response=client.get(format!("http://{address}{path}"))
                .send().await.unwrap();
            assert_eq!(response.status(),reqwest::StatusCode::OK,
                "core route unexpectedly unavailable: {path}");
        }
        for path in ["/api/mcp","/api/channels","/api/oauth/providers","/api/skills"]{
            let response=client.get(format!("http://{address}{path}"))
                .send().await.unwrap();
            assert_eq!(response.status(),reqwest::StatusCode::NOT_IMPLEMENTED,
                "disabled integration route unexpectedly exposed: {path}");
        }
        server.abort();
        drop(engine);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}

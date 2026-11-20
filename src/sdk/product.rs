//! Optional product-facing SDK methods: sessions, profiles, rooms, memory,
//! skill catalogs and integration discovery. Does not create another Run loop.
use super::*;
use crate::group::{NewRoom,Room,Say};
use axum::Router;

pub(super) fn apply_pending_product_restore(settings:&Settings)->Result<(),String>{
    crate::frontend_system::apply_pending(settings)
}

impl Dominion {
    pub async fn open(settings: Settings) -> Result<Self,String> {
        // Apply an explicitly staged, authenticated snapshot while no native
        // stores have opened yet; a browser reload alone is not a restore.
        product::apply_pending_product_restore(&settings)?;
        tokio::fs::create_dir_all(&settings.workspace_dir).await.map_err(|e|e.to_string())?;
        let store=crate::product::bootstrap::open_run_repository(&settings).await?;
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await?);
        let engine=Engine::new(settings,store,groups)?;
        // Reconcile durable parent/child settlement after a previous process
        // died between a child completion and the parent wake notification.
        let (suspended,safe_to_resume)=engine.store.recovery_candidates().await;
        for id in suspended {engine.settle_and_wake_ancestors(id).await?;}
        // Restart only work for which recovery proved there was no unfinished
        // mutating side effect. A previously approved call that never reached
        // the execution-intent checkpoint is also safe to start.
        for id in safe_to_resume {
            let archived=engine.store.get(id).await.is_some_and(|run|
                run.events.iter().any(|event|matches!(event.kind.as_str(),
                    "reserved_unlaunched"|"strategy_candidate"
                    |"strategy_trials_launched"|"strategy_trials_partial")));
            if archived {
                engine.store.mutate(id,|run|{
                    run.status=RunStatus::Paused;
                    run.event("archived_run_quarantined",
                        "Retired execution cannot be replayed after restart",None);
                    Ok(())
                }).await?;
                continue;
            }
            engine.launch(id).await?;
        }
        Ok(Self{engine})
    }

    /// HTTP is a product adapter, not a requirement of the native SDK.
    pub fn http_router(&self)->Router{crate::api::router(Arc::clone(&self.engine))}

    /// Main frontend and native SDK share the same durable profile/session
    /// records. These entries are presentation data; runs still execute only
    /// through the independent Engine and Store.
    pub async fn chat_sessions(&self)->Vec<crate::frontend_state::UiSession>{
        self.engine.frontend.sessions().await
    }
    pub async fn chat_session(&self,id:&str)->Option<crate::frontend_state::UiSession>{
        self.engine.frontend.session(id).await
    }
    pub async fn agent_profiles(&self)->Vec<crate::frontend_state::UiAgent>{
        self.engine.frontend.agents().await
    }
    pub async fn owner_profile(&self)->crate::profile::OwnerProfile{
        self.engine.owner_profile.get().await
    }
    pub async fn save_owner_profile(&self,req:crate::profile::NewOwnerProfile)
        ->Result<crate::profile::OwnerProfile,String>{
        self.engine.owner_profile.save(req).await
    }
    pub async fn create_group(&self,req:NewRoom)->Result<Room,String>{self.engine.groups.create(req).await}
    pub async fn say_group(&self,id:Uuid,input:Say)->Result<Room,String>{self.engine.groups.say(id,input.content).await}
    pub async fn read_subagent_file(&self,parent:Uuid,child:Uuid,path:&str)->Result<String,String>{
        self.engine.read_subagent_file(parent,child,path).await
    }
    pub async fn group_turn(&self,id:Uuid)->Result<Room,String>{
        let selected=self.engine.model.with_session_affinity(
            &format!("dominion-room-{id}"));
        self.engine.groups.next(&selected,id).await
    }
    /// Explicitly retrieve verified procedures from successful user-driven runs.
    /// No recursive self-modification, trial scheduler or background learning.
    pub async fn search_strategies(&self,repository:&str,goal:&str,max:usize)
        ->Vec<crate::strategy::StrategyEvidence>{
        let records=self.engine.store.strategy_evidence(repository).await;
        crate::strategy::rank(records,repository,goal,max)
    }
    pub async fn list_skills(&self,run:Uuid)->Result<Vec<crate::skills::SkillInfo>,String>{
        self.get(run).await?;
        crate::skills::list(&self.engine.settings,run).await
    }
    pub async fn load_skill(&self,run:Uuid,name:&str)->Result<String,String>{
        self.get(run).await?;
        crate::skills::load(&self.engine.settings,run,name).await
    }
    pub async fn remember(&self,scope:&str,title:&str,content:&str)->Result<crate::knowledge::MemoryEntry,String>{
        self.engine.knowledge.remember(scope,title,content).await
    }
    pub async fn search_memory(&self,scope:&str,query:&str)->Vec<crate::knowledge::MemoryEntry>{
        self.engine.knowledge.search(scope,query,20).await
    }
    pub async fn list_memory(&self,scope:&str)->Vec<crate::knowledge::MemoryEntry>{
        self.engine.knowledge.list(scope).await
    }
    pub async fn memory_scopes(&self)->Vec<String>{
        self.engine.knowledge.scopes().await
    }
    pub async fn revise_memory(&self,scope:&str,id:Uuid,title:&str,content:&str)
        ->Result<crate::knowledge::MemoryEntry,String>{
        self.engine.knowledge.revise(scope,id,title,content).await
    }
    pub async fn forget_memory(&self,scope:&str,id:Uuid)->Result<(),String>{
        self.engine.knowledge.forget(scope,id).await
    }
    pub async fn tool_catalog(&self)->Vec<Value>{
        let mut tools=tools::definitions(self.engine.settings.shell_enabled);
        tools.extend(self.engine.addons.tool_definitions(&self.engine.settings).await);
        // The SDK and model must see the same operator-owned tool mask.
        tools.retain(|tool|{
            let name=tool.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
            !self.engine.settings.denied_tools.iter().any(|blocked|blocked==name)
        });
        tools
    }
}

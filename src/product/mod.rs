//! Product-level services for the Dominion application. These are adapters
//! around the native Run engine, not the execution kernel or a second scheduler.
//! The Run scheduler remains native; the surrounding presentation, room,
//! profile, catalog and stored-memory facilities are product services.
pub(crate) mod bootstrap;
pub mod config;
pub mod group;
pub mod native_api;
pub mod build_identity;
pub mod strategy;
pub mod knowledge;
pub mod profile;
pub mod provider_registry;
pub mod frontend_state;
pub mod frontend_api;
pub mod frontend_codex_login;
pub mod frontend_growth;
pub mod frontend_huggingface;
pub mod frontend_local_models;
pub mod frontend_model_downloads;
pub mod frontend_native_picker;
pub mod frontend_personas;
pub mod frontend_provider_public;
pub mod frontend_providers;
pub mod frontend_rooms;
pub mod frontend_system;
pub(crate) mod frontend_public_http;

use crate::{addons,config::Settings,domain::Run,group::Groups,knowledge::Knowledge,
    profile::OwnerProfileStore,frontend_state::FrontendStore,
    provider::ModelClient,provider_registry::Registry,mcp};
use std::{collections::HashMap,sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

/// An execution-local snapshot of external owner notes. The process-local
/// knowledge generation invalidates it after any successful owner edit.
struct FocusedMemory{
    revision:u64,
    scope:String,
    query:String,
    body:String,
}

/// Application-owned services are grouped behind one non-execution host.
/// Public fields preserve source compatibility for existing UI/SDK adapters.
pub struct ProductHost {
    pub groups:Arc<Groups>,
    pub addons:Arc<addons::Host>,
    pub knowledge:Arc<Knowledge>,
    pub owner_profile:Arc<OwnerProfileStore>,
    pub frontend:Arc<FrontendStore>,
    pub providers:Arc<Registry>,
    focused_memory:Mutex<HashMap<Uuid,FocusedMemory>>,
}
impl ProductHost {
    /// Product catalog and local model metadata determine whether token
    /// pricing is *unknown*. The execution kernel only records the receipt.
    pub fn pricing_unknown(&self,settings:&Settings,provider:&str,model:&str)->bool{
        if provider.is_empty()||provider=="dominion-native"{
            let selected=&settings.main;
            let local_library=crate::frontend_local_models::native_override(
                settings,model).is_some();
            let local_origin=reqwest::Url::parse(&selected.base_url).ok()
                .and_then(|url|url.host_str().map(|host|
                    matches!(host,"localhost"|"127.0.0.1"|"::1")))
                .unwrap_or(false);
            !local_library&&!local_origin&&(selected.input_krw_per_million==0.0
                ||selected.output_krw_per_million==0.0)
        }else{
            !self.providers.get(provider).is_some_and(|entry|entry.local)
        }
    }

    #[cfg(test)]
    pub(crate) async fn recall_cache_query(&self,id:Uuid)->Option<String>{
        self.focused_memory.lock().await.get(&id).map(|entry|entry.query.clone())
    }
    #[cfg(test)]
    pub(crate) async fn recall_cache_len(&self)->usize{
        self.focused_memory.lock().await.len()
    }

    /// Product-owned scoped recall is released when a native Run terminates.
    /// The execution kernel does not own user-memory indexes or cache state.
    pub(crate) async fn release_run_context(&self,id:Uuid){
        self.focused_memory.lock().await.remove(&id);
    }
    pub fn open(settings:&Settings,groups:Arc<Groups>,mcp:Arc<mcp::Mcp>)
        ->Result<Self,String>{
        Ok(Self{
            groups,
            addons:addons::Host::open(settings,mcp)?,
            knowledge:Arc::new(Knowledge::open(settings.data_dir.clone())?),
            owner_profile:Arc::new(OwnerProfileStore::open(settings.data_dir.clone())?),
            frontend:Arc::new(FrontendStore::open(settings.data_dir.clone(),&settings.main.name)?),
            providers:Arc::new(Registry::open(settings.data_dir.clone())?),
            focused_memory:Mutex::new(HashMap::new()),
        })
    }
/// Search once per Run instruction/knowledge revision rather than
/// re-lowercasing all persisted notes for every model turn. The last two
/// owner corrections can retarget memory; model/tool text cannot.
pub(crate) async fn focused_notes(&self,run:&Run)->String{
    let scope=run.repository.as_deref().unwrap_or("global");
    let changes=run.messages.iter().rev().filter_map(|message|{
        if message["role"]!="user"{return None;}
        message["content"].as_str()?.strip_prefix("New user instruction:")
    }).take(2).collect::<Vec<_>>();
    let goal=run.instruction.chars().take(1600).collect::<String>();
    let owner=changes.into_iter().rev()
        .map(|change|change.chars().take(450).collect::<String>())
        .collect::<Vec<_>>().join(" ");
    // Only classified persisted failures are used as retrieval control
    // signals. Arbitrary tool output never becomes a trusted query or
    // new instruction. A changed failure invalidates cached recall.
    let failure=crate::repair_guidance::advise(run)
        .map(|advice|advice.kind.replace('_'," "))
        .unwrap_or_default();
    let query=format!("goal:{goal}\nowner:{owner}\nfailure:{failure}");
    let revision=self.knowledge.revision();
    {
        let cache=self.focused_memory.lock().await;
        if let Some(saved)=cache.get(&run.id){
            if saved.revision==revision&&saved.scope==scope&&saved.query==query{
                return saved.body.clone();
            }
        }
    }
    let limit=if failure.is_empty(){4}else{2};
    let notes=self.knowledge.search_context(scope,&goal,&owner,&failure,limit).await;
    let body=notes.iter().map(|note|format!("- {}: {}",
        note.title,note.content.chars().take(650).collect::<String>()))
        .collect::<Vec<_>>().join("\n");
    // A concurrent memory edit invalidates the snapshot on the next
    // model turn. Never mark a stale result as freshly retrieved.
    if self.knowledge.revision()==revision{
        self.focused_memory.lock().await.insert(run.id,FocusedMemory{
            revision,scope:scope.to_string(),query,body:body.clone(),
        });
    }
    body
}

    pub fn model_for_selection(&self,settings:&Settings,base:&ModelClient,provider:&str,model:&str)->Result<ModelClient,String>{
        if provider.is_empty()||provider=="dominion-native" {
            if let Some(local)=crate::frontend_local_models::native_override(&settings,model){
                return base.with_primary_protocol(local,"openai");
            }
        }
        let settings=self.providers.resolve(provider,model,&settings.main)?;
        if provider.is_empty()||provider=="dominion-native"{
            Ok(base.clone())
        }else{
            let record=self.providers.get(provider).ok_or("provider registry entry unavailable")?;
            base.with_primary_protocol(settings,&record.kind)
        }
    }

}

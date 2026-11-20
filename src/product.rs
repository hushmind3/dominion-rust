//! Product-level services for the Dominion application. These are adapters
//! around the native Run engine, not the execution kernel or a second scheduler.
//! A model-only agent can execute without any direct dependency on the GUI,
//! group/room system, profile editor, durable memory catalog, or MCP registry.
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
    pub(crate) focused_memory:Mutex<HashMap<Uuid,FocusedMemory>>,
}
impl ProductHost {
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

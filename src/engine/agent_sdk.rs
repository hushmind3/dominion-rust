//! Native Rust embedding interface. Uses the SAME chat runtime, SQLite,
//! tool permissions, approvals, cancellation and model providers as /api/chat.
use crate::{engine::chat, state::AppState};
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

#[derive(Clone)]
pub struct DominionSdk {
    state: AppState,
}

impl DominionSdk {
    pub fn new(state: AppState) -> Self { Self { state } }

    pub fn start_thread(&self, options: &Value) -> Result<Value, String> {
        if !options.is_object() { return Err("SDK thread options must be an object".into()); }
        let id = uuid::Uuid::new_v4().to_string();
        let title = options["title"].as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("SDK thread")
            .chars().take(200).collect::<String>();
        let row = json!({
            "id":id,"title":title,
            "agentId":options["agentId"].as_str(),
            "providerId":options["providerId"].as_str(),
            "model":options["model"].as_str(),
            "archived":false,"origin":"native_sdk",
        });
        self.state.store.put_if_absent("session",&row)
            .ok_or_else(|| "Could not persist SDK thread".into())
    }

    pub fn resume_thread(&self, id: &str) -> Result<Value, String> {
        let session=self.state.store.get("session",id)
            .ok_or_else(|| "SDK session not found".to_string())?;
        if session["roomId"].is_string() {
            return Err("Group room sessions have their own owner-message API".into());
        }
        if session["archived"] == true { return Err("SDK session is archived".into()); }
        Ok(session)
    }

    pub fn history(&self, id: &str) -> Result<Vec<Value>, String> {
        self.resume_thread(id)?;
        self.state.store.try_messages(id)
            .map_err(|error|format!("SDK thread history: {error}"))
    }

    fn request(&self, id: &str, input: &Value) -> Result<chat::TurnRequest, String> {
        self.resume_thread(id)?;
        if !input.is_object() { return Err("SDK input must be an object".into()); }
        let mut body=input.clone();
        // Never let caller spoof another thread through the nested body.
        body["sessionId"]=json!(id);
        chat::TurnRequest::from_value(&body)
    }

    /// Return the real final message. No second inference engine or model CLI.
    pub async fn run(&self, id:&str, input:&Value) -> Result<Option<Value>,String> {
        let request=self.request(id,input)?;
        let (tx,_rx)=mpsc::unbounded_channel();
        chat::run_turn(&self.state,&request,&tx).await
    }

    /// Live events come from the real Rust execution loop, not synthetic
    /// progress counters. Disconnecting the receiver does not cancel the job.
    pub fn run_streamed(&self,id:&str,input:&Value)
        -> Result<UnboundedReceiver<Value>,String> {
        let request=self.request(id,input)?;
        let state=self.state.clone();
        let (tx,rx)=mpsc::unbounded_channel();
        tokio::spawn(async move {
            let result=chat::run_turn(&state,&request,&tx).await;
            finish_stream(&tx,result,request.session_id.as_deref().unwrap_or(""));
        });
        Ok(rx)
    }

    pub fn cancel(&self,id:&str)->Result<(),String> {
        self.resume_thread(id)?;
        self.state.cancel_session(id);
        Ok(())
    }
}

fn finish_stream(tx:&UnboundedSender<Value>,result:Result<Option<Value>,String>,id:&str) {
    match result {
        Ok(message)=>{
            if let Some(message)=message {
                let _=tx.send(json!({"type":"assistant_saved",
                    "messageId":message["id"],"sessionId":message["session_id"]}));
            }
            let _=tx.send(json!({"type":"done","sessionId":id}));
        }
        Err(error)=>{
            let _=tx.send(json!({"type":"error","message":error}));
            let _=tx.send(json!({"type":"done","sessionId":id}));
        }
    }
}

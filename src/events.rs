use serde_json::{json, Value};
use tokio::sync::broadcast;

/// In-process event bus backing `GET /api/events`. The React client listens on
/// it to refresh lists when the engine writes records or messages.
#[derive(Clone)]
pub struct Bus {
    sender: broadcast::Sender<Value>,
}

impl Bus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(512);
        Self { sender }
    }

    pub fn emit(&self, value: Value) {
        let _ = self.sender.send(value);
    }

    pub fn record(&self, kind: &str, record: &Value) {
        self.emit(json!({ "type": "record", "kind": kind, "record": record }));
    }

    pub fn message(&self, message: &Value) {
        self.emit(json!({ "type": "message", "message": message }));
    }

    pub fn approval(&self, approval: &Value) {
        self.emit(json!({ "type": "approval", "approval": approval }));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.sender.subscribe()
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

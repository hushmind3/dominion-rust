use crate::db::Store;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::broadcast;

/// In-process event bus backing `GET /api/events`, backed by a bounded durable
/// sequence (`event_log`). A client that reconnects after `Lagged` or a
/// disconnect can replay from its last `seq` instead of silently missing writes.
#[derive(Clone)]
pub struct Bus {
    sender: broadcast::Sender<Value>,
    store: Store,
    seq: Arc<AtomicU64>,
}

impl Bus {
    pub fn new(store: Store) -> Self {
        let seq = store.last_event_seq();
        let (sender, _) = broadcast::channel(512);
        Self {
            sender,
            store,
            seq: Arc::new(AtomicU64::new(seq)),
        }
    }

    pub fn emit(&self, mut value: Value) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        value["seq"] = json!(seq);
        self.store.append_event(seq, &value);
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

    /// Replays durable events after `after` (exclusive). Used on reconnect.
    pub fn replay(&self, after: u64, limit: usize) -> Vec<Value> {
        self.store.events_after(after, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn events_are_durable_and_replayable_after_a_disconnect() {
        let store = store();
        let bus = Bus::new(store.clone());
        bus.emit(json!({"type":"first"}));
        bus.emit(json!({"type":"second"}));
        bus.emit(json!({"type":"third"}));
        let replay = bus.replay(1, 10);
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0]["seq"], 2);
        assert_eq!(replay[0]["type"], "second");
        assert_eq!(replay[1]["seq"], 3);
        assert!(bus.replay(3, 10).is_empty());

        // A restarted process continues the sequence instead of reusing ids.
        let restarted = Bus::new(store);
        restarted.emit(json!({"type":"fourth"}));
        assert_eq!(restarted.replay(3, 10)[0]["seq"], 4);
    }

    #[test]
    fn the_log_is_bounded_without_losing_recent_events() {
        let store = store();
        let bus = Bus::new(store);
        for index in 0..600 {
            bus.emit(json!({"type":"tick","index":index}));
        }
        let recent = bus.replay(0, 1_000);
        assert!(!recent.is_empty());
        assert_eq!(recent.last().unwrap()["index"], 599);
        assert!(recent.len() <= 600);
    }
}

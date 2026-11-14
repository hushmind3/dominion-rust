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

    /// Drain the bounded SQLite event history in successive windows. The HTTP
    /// SSE endpoint must not stop at the first 500 rows and silently skip all
    /// remaining historical events when it transitions to live broadcast.
    /// 4096 exceeds the maximum retained log (~2256 rows), but also bounds
    /// a catch-up attempt if writers keep producing new events continuously.
    pub fn replay_available(&self, after: u64) -> Vec<Value> {
        let mut cursor = after;
        let mut events = Vec::new();
        while events.len() < 4096 {
            let batch = self.replay(cursor, 500);
            let count = batch.len();
            if count == 0 { break; }
            let previous = cursor;
            for event in batch {
                let Some(seq) = event["seq"].as_u64() else { continue };
                if seq > cursor {
                    cursor = seq;
                    events.push(event);
                }
            }
            if count < 500 || cursor == previous { break; }
        }
        events
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
    fn reconnect_replays_all_retained_events_in_order_without_500_row_holes() {
        let bus=Bus::new(store());
        for index in 0..900 {
            bus.emit(json!({"type":"step","index":index}));
        }
        let recovered=bus.replay_available(0);
        assert_eq!(recovered.len(),900);
        assert_eq!(recovered.first().unwrap()["seq"],1);
        assert_eq!(recovered.last().unwrap()["seq"],900);
        assert!(recovered.windows(2).all(|w| w[1]["seq"].as_u64()==
            w[0]["seq"].as_u64().map(|n| n+1)));
        assert_eq!(bus.replay_available(899).len(),1);
        assert!(bus.replay_available(900).is_empty());
    }

    #[test]
    fn reconnect_after_retention_pruning_starts_at_actual_first_available() {
        let bus=Bus::new(store());
        for index in 0..2800 {
            bus.emit(json!({"type":"step","index":index}));
        }
        let recovered=bus.replay_available(1);
        assert!(!recovered.is_empty());
        assert!(recovered.first().unwrap()["seq"].as_u64().unwrap()>1);
        assert_eq!(recovered.last().unwrap()["seq"],2800);
        assert!(recovered.windows(2).all(|w| w[1]["seq"].as_u64()==
            w[0]["seq"].as_u64().map(|n| n+1)));
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

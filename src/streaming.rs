use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Event {
    NewStatus {
        instance_id: Uuid,
        author_id: i64,
        is_public: bool,
        is_direct: bool,
        status_id: i64,
        hashtags: Vec<String>,
        payload: Arc<String>,
    },
    Notification {
        for_account_id: i64,
        payload: Arc<String>,
    },
    DeleteStatus {
        instance_id: Uuid,
        status_id: i64,
    },
}

#[derive(Clone)]
pub struct StreamBus {
    tx: broadcast::Sender<Arc<Event>>,
}

impl StreamBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(Arc::new(event));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.tx.subscribe()
    }
}

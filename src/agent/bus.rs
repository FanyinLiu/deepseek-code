use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Message Bus
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A topic-based message bus for inter-agent communication.
///
/// Agents can publish messages to topics and subscribe to receive messages.
/// This enables coordination between parallel subagents without requiring
/// the parent agent to act as an intermediary for every exchange.
///
/// Example topics:
/// - `files.lock` — agents announce which files they are modifying
/// - `progress` — agents report progress updates
/// - `findings` — agents share discoveries
/// - `errors` — agents report errors that may affect others
#[derive(Clone)]
pub struct MessageBus {
    inner: Arc<Mutex<HashMap<String, broadcast::Sender<Message>>>>,
    /// Best-effort lock table: path -> (`agent_id`, locked)
    locks: Arc<Mutex<std::collections::HashMap<String, String>>>,
    default_capacity: usize,
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new(64)
    }
}

impl MessageBus {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(Mutex::new(std::collections::HashMap::new())),
            default_capacity: capacity,
        }
    }

    /// Publish a message to a topic.
    ///
    /// If the topic does not exist, it is created automatically.
    /// Returns the number of subscribers that received the message.
    pub fn publish(&self, topic: &str, sender: &str, payload: MessagePayload) -> usize {
        let msg = Message {
            topic: topic.to_string(),
            sender_id: sender.to_string(),
            payload,
            timestamp: Utc::now(),
        };

        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("MessageBus inner mutex poisoned: {}", e);
                return 0;
            }
        };
        let sender = map
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(self.default_capacity).0);
        if let Ok(n) = sender.send(msg) {
            n
        } else {
            tracing::warn!(
                "MessageBus publish dropped for topic '{}' — no active subscribers",
                topic
            );
            0
        }
    }

    /// Subscribe to a topic.
    ///
    /// Returns a receiver that yields messages published to this topic.
    /// If the topic does not exist, it is created.
    pub fn subscribe(&self, topic: &str) -> broadcast::Receiver<Message> {
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("MessageBus inner mutex poisoned: {}", e);
                // Return a dummy receiver that will never receive anything
                let (_, rx) = broadcast::channel(1);
                return rx;
            }
        };
        let sender = map
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(self.default_capacity).0);
        sender.subscribe()
    }

    /// Get the number of active subscribers for a topic.
    pub fn subscriber_count(&self, topic: &str) -> usize {
        let map = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.get(topic).map_or(0, broadcast::Sender::receiver_count)
    }

    /// List all active topics.
    pub fn topics(&self) -> Vec<String> {
        let map = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.keys().cloned().collect()
    }

    /// Publish a file lock announcement.
    ///
    /// Agents should call this before modifying a file so that parallel
    /// agents can avoid conflicts.
    pub fn announce_file_lock(&self, agent_id: &str, file_path: &str) -> usize {
        if let Ok(mut locks) = self.locks.lock() {
            locks.insert(file_path.to_string(), agent_id.to_string());
        } else {
            tracing::error!("MessageBus locks mutex poisoned");
        }
        self.publish(
            "files.lock",
            agent_id,
            MessagePayload::FileLock {
                path: file_path.to_string(),
                action: "lock".to_string(),
            },
        )
    }

    /// Publish a file unlock announcement.
    pub fn announce_file_unlock(&self, agent_id: &str, file_path: &str) -> usize {
        if let Ok(mut locks) = self.locks.lock() {
            locks.remove(file_path);
        } else {
            tracing::error!("MessageBus locks mutex poisoned");
        }
        self.publish(
            "files.lock",
            agent_id,
            MessagePayload::FileLock {
                path: file_path.to_string(),
                action: "unlock".to_string(),
            },
        )
    }

    /// Publish a progress update.
    #[must_use]
    pub fn announce_progress(&self, agent_id: &str, step: u32, total: u32, detail: &str) -> usize {
        self.publish(
            "progress",
            agent_id,
            MessagePayload::Progress {
                step,
                total,
                detail: detail.to_string(),
            },
        )
    }

    /// Check if any agent has locked a given file.
    pub fn is_file_locked(&self, file_path: &str) -> bool {
        let locks = self
            .locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        locks.contains_key(file_path)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Message Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
pub struct Message {
    pub topic: String,
    pub sender_id: String,
    pub payload: MessagePayload,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum MessagePayload {
    /// A file lock/unlock announcement.
    FileLock {
        path: String,
        action: String, // "lock" or "unlock"
    },
    /// A progress update.
    Progress {
        step: u32,
        total: u32,
        detail: String,
    },
    /// A shared finding or discovery.
    Finding { title: String, content: String },
    /// An error that may affect other agents.
    Error { message: String, recoverable: bool },
    /// Raw text payload.
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publish_and_subscribe() {
        let bus = MessageBus::new(16);
        let mut rx = bus.subscribe("test.topic");

        let count = bus.publish(
            "test.topic",
            "agent-1",
            MessagePayload::Text("hello".to_string()),
        );
        assert_eq!(count, 1);

        let msg = rx.try_recv().unwrap();
        assert_eq!(msg.topic, "test.topic");
        assert_eq!(msg.sender_id, "agent-1");
        match msg.payload {
            MessagePayload::Text(t) => assert_eq!(t, "hello"),
            _ => panic!("Expected Text payload"),
        }
    }

    #[test]
    fn test_file_lock_tracking() {
        let bus = MessageBus::new(16);
        bus.announce_file_lock("agent-a", "src/main.rs");
        assert!(bus.is_file_locked("src/main.rs"));

        bus.announce_file_unlock("agent-a", "src/main.rs");
        assert!(!bus.is_file_locked("src/main.rs"));
    }

    #[test]
    fn test_progress_announcement() {
        let bus = MessageBus::new(16);
        let mut rx = bus.subscribe("progress");

        let _ = bus.announce_progress("agent-1", 2, 5, "Reading files");

        let msg = rx.try_recv().unwrap();
        match msg.payload {
            MessagePayload::Progress {
                step,
                total,
                detail,
            } => {
                assert_eq!(step, 2);
                assert_eq!(total, 5);
                assert_eq!(detail, "Reading files");
            }
            _ => panic!("Expected Progress payload"),
        }
    }

    #[test]
    fn test_multiple_subscribers() {
        let bus = MessageBus::new(16);
        let _rx1 = bus.subscribe("broadcast");
        let _rx2 = bus.subscribe("broadcast");

        let count = bus.publish(
            "broadcast",
            "sender",
            MessagePayload::Text("all".to_string()),
        );
        assert_eq!(count, 2);
    }
}

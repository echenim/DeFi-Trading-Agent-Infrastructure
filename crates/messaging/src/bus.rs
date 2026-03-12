use common::messages::Message;
use common::MessageBusError;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};

const DEFAULT_CAPACITY: usize = 4096;

/// Async message bus using tokio broadcast channels.
///
/// All agents publish and subscribe through the same bus.
/// Each subscriber gets its own copy of every message.
#[derive(Clone)]
pub struct MessageBus {
    sender: Arc<broadcast::Sender<Message>>,
}

impl MessageBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Publish a message to all subscribers.
    pub fn publish(&self, msg: Message) -> Result<usize, MessageBusError> {
        let receivers = self
            .sender
            .send(msg)
            .map_err(|e| MessageBusError::SendFailed(e.to_string()))?;
        debug!(receivers, "message published");
        Ok(receivers)
    }

    /// Create a new subscriber that receives all future messages.
    pub fn subscribe(&self) -> BusSubscriber {
        BusSubscriber {
            receiver: self.sender.subscribe(),
        }
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// A subscriber handle for receiving messages from the bus.
pub struct BusSubscriber {
    receiver: broadcast::Receiver<Message>,
}

impl BusSubscriber {
    /// Receive the next message, waiting if none available.
    pub async fn recv(&mut self) -> Result<Message, MessageBusError> {
        loop {
            match self.receiver.recv().await {
                Ok(msg) => return Ok(msg),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(missed = n, "subscriber lagged, skipping missed messages");
                    // Continue receiving — don't error out on lag
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(MessageBusError::ChannelClosed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::messages::{Envelope, MarketSignal, SignalType};

    fn make_signal_message() -> Message {
        Message::Signal(Envelope::new(MarketSignal {
            signal_type: SignalType::NewBlock { block_number: 42 },
            quotes: vec![],
            source_tx: None,
        }))
    }

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = MessageBus::with_default_capacity();
        let mut sub = bus.subscribe();

        bus.publish(make_signal_message()).unwrap();

        let msg = sub.recv().await.unwrap();
        assert!(matches!(msg, Message::Signal(_)));
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = MessageBus::with_default_capacity();
        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();

        bus.publish(make_signal_message()).unwrap();

        let msg1 = sub1.recv().await.unwrap();
        let msg2 = sub2.recv().await.unwrap();

        assert!(matches!(msg1, Message::Signal(_)));
        assert!(matches!(msg2, Message::Signal(_)));
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let bus = MessageBus::with_default_capacity();
        assert_eq!(bus.subscriber_count(), 0);

        let _sub1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        let _sub2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        drop(_sub1);
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[tokio::test]
    async fn test_no_subscribers_returns_error() {
        let bus = MessageBus::with_default_capacity();
        let result = bus.publish(make_signal_message());
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_late_subscriber_misses_old_messages() {
        let bus = MessageBus::with_default_capacity();
        let _sub_early = bus.subscribe(); // need at least one subscriber for send to work

        bus.publish(make_signal_message()).unwrap();

        let mut sub_late = bus.subscribe();

        // Publish a new one
        bus.publish(make_signal_message()).unwrap();

        let msg = sub_late.recv().await.unwrap();
        assert!(matches!(msg, Message::Signal(_)));
    }
}

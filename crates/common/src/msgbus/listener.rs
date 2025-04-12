// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 2Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use bytes::Bytes;
use futures::stream::Stream;

use super::BusMessage;

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct MessageBusListener {
    tx: tokio::sync::mpsc::UnboundedSender<BusMessage>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<BusMessage>>,
}

impl Default for MessageBusListener {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBusListener {
    /// Creates a new [`MessageBusListener`] instance.
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();
        Self { tx, rx: Some(rx) }
    }

    /// Returns whether the listener is closed.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Closes the listener.
    pub fn close(&mut self) {
        log::debug!("Closing");

        // Drop receiver
        if let Some(rx) = self.rx.take() {
            drop(rx);
        }

        // Drop sender
        let (new_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let _ = std::mem::replace(&mut self.tx, new_tx);

        log::debug!("Closed");
    }

    /// Publishes a message with the given `topic` and `payload`.
    pub fn publish(&self, topic: String, payload: Bytes) {
        let msg = BusMessage { topic, payload };
        if let Err(e) = self.tx.send(msg) {
            log::error!("Failed to send message: {e}");
        }
    }

    /// Gets the stream receiver for this instance.
    pub fn get_stream_receiver(
        &mut self,
    ) -> anyhow::Result<tokio::sync::mpsc::UnboundedReceiver<BusMessage>> {
        self.rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
    }

    /// Streams messages arriving on the receiver channel.
    pub fn stream(
        mut stream_rx: tokio::sync::mpsc::UnboundedReceiver<BusMessage>,
    ) -> impl Stream<Item = BusMessage> + 'static {
        async_stream::stream! {
            while let Some(msg) = stream_rx.recv().await {
                yield msg;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::StreamExt;
    use tokio::sync::mpsc;

    use super::*;

    #[tokio::test]
    async fn test_new_listener() {
        let listener = MessageBusListener::new();
        assert!(!listener.is_closed());
    }

    #[tokio::test]
    async fn test_close_listener() {
        let mut listener = MessageBusListener::new();
        listener.close();
        assert!(listener.is_closed());
    }

    #[tokio::test]
    async fn test_publish_and_receive() {
        let mut listener = MessageBusListener::new();

        // Get the receiver
        let rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");

        // Create a simple channel to collect messages
        let (notify_tx, mut notify_rx) = mpsc::channel::<()>(1);

        // Spawn a task to process messages
        let handle = tokio::spawn(async move {
            let stream = MessageBusListener::stream(rx);
            futures::pin_mut!(stream);
            let msg = stream.next().await.expect("No message received");

            assert_eq!(msg.topic, "test-topic");
            assert_eq!(msg.payload.as_ref(), b"test-payload");
            notify_tx.send(()).await.unwrap();
        });

        // Publish a message
        listener.publish("test-topic".to_string(), Bytes::from("test-payload"));

        // Wait for the message to be processed
        tokio::select! {
            _ = notify_rx.recv() => {},
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                panic!("Timeout waiting for message");
            }
        }

        // Clean up
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let mut listener = MessageBusListener::new();
        let rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");

        let topics = vec!["topic1", "topic2", "topic3"];
        let payloads = vec!["payload1", "payload2", "payload3"];

        let topics_clone = topics.clone();
        let payloads_clone = payloads.clone();

        // Spawn a task to collect messages
        let handle = tokio::spawn(async move {
            let stream = MessageBusListener::stream(rx);
            futures::pin_mut!(stream);

            let mut received = Vec::new();

            for _ in 0..3 {
                if let Some(msg) = stream.next().await {
                    received.push((msg.topic, String::from_utf8(msg.payload.to_vec()).unwrap()));
                }
            }

            // Verify all messages were received
            for i in 0..3 {
                assert!(
                    received
                        .contains(&(topics_clone[i].to_string(), payloads_clone[i].to_string()))
                );
            }

            received
        });

        // Publish messages
        for i in 0..3 {
            listener.publish(
                topics[i].to_string(),
                Bytes::from(payloads[i].as_bytes().to_vec()),
            );
        }

        // Wait for the task to complete and check result
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(1), handle)
            .await
            .expect("Test timed out")
            .expect("Task panicked");

        assert_eq!(result.len(), 3);
    }

    #[tokio::test]
    async fn test_stream_receiver_already_taken() {
        let mut listener = MessageBusListener::new();

        // First call should succeed
        let _rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");

        // Second call should fail
        assert!(listener.get_stream_receiver().is_err());
    }

    #[tokio::test]
    async fn test_publish_after_close() {
        let mut listener = MessageBusListener::new();

        let _rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");

        listener.close();
        assert!(listener.is_closed());

        // Publishing should log an error but not panic
        listener.publish("test-topic".to_string(), Bytes::from("test-payload"));
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Message bus listener for live trading using tokio channels.

use bytes::Bytes;
use futures::{StreamExt, stream::Stream};
use ustr::Ustr;

use crate::{
    enums::SerializationEncoding,
    msgbus::{BusMessage, BusPayloadType, MStr, Topic},
};

#[cfg_attr(feature = "python", pyo3::pyclass(module = "nautilus_trader.common"))]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
#[derive(Debug)]
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

        self.rx = None;
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        self.tx = tx;

        log::debug!("Closed");
    }

    /// Publishes a message with the given `topic` and `payload`.
    pub fn publish<T: Into<MStr<Topic>>>(&self, topic: T, payload: Bytes) {
        let topic = topic.into();

        // Listener messages are untyped, so they use default bus headers.
        let msg = BusMessage::new(
            *topic,
            BusPayloadType::Custom(Ustr::default()),
            payload,
            SerializationEncoding::default(),
        );

        if let Err(e) = self.tx.send(msg) {
            log::error!("Failed to send message: {e}");
        }
    }

    /// Gets the stream receiver for this instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream receiver has already been taken.
    pub fn get_stream_receiver(
        &mut self,
    ) -> anyhow::Result<tokio::sync::mpsc::UnboundedReceiver<BusMessage>> {
        self.rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
    }

    /// Streams messages arriving on the receiver channel.
    pub fn stream(
        stream_rx: tokio::sync::mpsc::UnboundedReceiver<BusMessage>,
    ) -> impl Stream<Item = BusMessage> + 'static {
        futures::stream::unfold(stream_rx, |mut rx| async {
            rx.recv().await.map(|msg| (msg, rx))
        })
        .fuse()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::StreamExt;
    use ustr::Ustr;

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
        assert_publish(Ustr::from("test-topic"), "test-topic", "test-payload").await;
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let mut listener = MessageBusListener::new();
        let rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");

        let topics = ["topic1", "topic2", "topic3"];
        let payloads = ["payload1", "payload2", "payload3"];

        let stream = MessageBusListener::stream(rx);
        futures::pin_mut!(stream);
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(1), async {
            let mut received = Vec::new();

            for _ in 0..3 {
                if let Some(msg) = stream.next().await {
                    received.push((msg.topic, String::from_utf8(msg.payload.to_vec()).unwrap()));
                }
            }

            received
        });

        // Publish messages
        for i in 0..3 {
            listener.publish(
                Ustr::from(topics[i]),
                Bytes::from(payloads[i].as_bytes().to_vec()),
            );
        }

        let result = result.await.expect("Test timed out");

        let expected = topics
            .iter()
            .zip(payloads)
            .map(|(topic, payload)| (Ustr::from(topic), payload.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(result, expected);
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

        let rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");
        let stream = MessageBusListener::stream(rx);
        futures::pin_mut!(stream);

        listener.close();
        assert!(listener.is_closed());
        assert!(stream.next().await.is_none());
        assert!(stream.next().await.is_none());

        // Publishing should log an error but not panic
        listener.publish(Ustr::from("test-topic"), Bytes::from("test-payload"));
    }

    #[tokio::test]
    async fn test_publish_with_mstr_topic() {
        let topic = MStr::<Topic>::from("mstr-topic");
        assert_publish(topic, "mstr-topic", "mstr-payload").await;
    }

    #[tokio::test]
    async fn test_publish_with_string_topic() {
        assert_publish("string-topic", "string-topic", "string-payload").await;
    }

    async fn assert_publish<T: Into<MStr<Topic>>>(topic: T, expected_topic: &str, payload: &str) {
        let mut listener = MessageBusListener::new();
        let mut rx = listener
            .get_stream_receiver()
            .expect("Failed to get stream receiver");

        listener.publish(topic, Bytes::copy_from_slice(payload.as_bytes()));
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Test timed out")
            .expect("No message received");

        assert_eq!(msg.topic, expected_topic);
        assert_eq!(msg.payload_type, BusPayloadType::Custom(Ustr::default()));
        assert_eq!(msg.payload.as_ref(), payload.as_bytes());
        assert_eq!(msg.encoding, SerializationEncoding::Json);
    }
}

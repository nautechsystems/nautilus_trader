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

//! Transport abstraction layer for WebSocket backends.
//!
//! Defines the backend-agnostic surface that higher layers in `nautilus-network`
//! consume (the reconnecting client, auth tracker, subscription manager, and adapter
//! crates):
//!
//! - [`Message`]: neutral WebSocket message enum.
//! - [`TransportError`]: neutral error type.
//! - [`WsTransport`]: `Stream` plus `Sink` trait for backend implementations.
//!
//! The `tokio-tungstenite` backend is always compiled (its conversions and adapter
//! live in [`tungstenite`]). The `sockudo-ws` backend is gated behind the
//! `transport-sockudo` feature and lives in the `sockudo` submodule; when enabled
//! it can be selected at runtime via `WebSocketConfig.backend`.

pub mod error;
pub mod message;
pub mod stream;
pub mod tungstenite;

#[cfg(feature = "transport-sockudo")]
pub mod sockudo;

pub use error::TransportError;
pub use message::{CloseFrame, Message};
pub use stream::{BoxedWsTransport, WsTransport};

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use proptest::prelude::*;
    use rstest::rstest;
    #[cfg(feature = "transport-sockudo")]
    use sockudo_ws::protocol::Message as SockudoMessage;
    use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

    use super::*;

    fn message_strategy() -> impl Strategy<Value = Message> {
        prop_oneof![
            any::<String>().prop_map(Message::text),
            prop::collection::vec(any::<u8>(), 0..256).prop_map(Message::binary),
            prop::collection::vec(any::<u8>(), 0..=125)
                .prop_map(|bytes| Message::ping(Bytes::from(bytes))),
            prop::collection::vec(any::<u8>(), 0..=125)
                .prop_map(|bytes| Message::pong(Bytes::from(bytes))),
            prop::option::of((any::<u16>(), any::<String>())).prop_map(|frame| {
                Message::Close(frame.map(|(code, reason)| CloseFrame::new(code, reason)))
            }),
        ]
    }

    proptest! {
        #[rstest]
        fn message_conversions_round_trip(message in message_strategy()) {
            let tungstenite = TungsteniteMessage::try_from(message.clone()).unwrap();
            let tungstenite_round_trip = Message::from(tungstenite);
            prop_assert_eq!(&tungstenite_round_trip, &message);

            #[cfg(feature = "transport-sockudo")]
            {
                let sockudo = SockudoMessage::from(message.clone());
                prop_assert_eq!(Message::from(sockudo), message);
            }
        }

        #[rstest]
        fn invalid_text_conversion_is_backend_specific(
            bytes in prop::collection::vec(any::<u8>(), 1..256)
                .prop_filter("invalid UTF-8", |bytes| std::str::from_utf8(bytes).is_err())
        ) {
            let message = Message::Text(Bytes::from(bytes));
            let tungstenite = TungsteniteMessage::try_from(message.clone());

            prop_assert!(matches!(tungstenite, Err(TransportError::InvalidUtf8)));

            #[cfg(feature = "transport-sockudo")]
            {
                let sockudo = SockudoMessage::from(message.clone());
                prop_assert_eq!(Message::from(sockudo), message);
            }
        }
    }
}

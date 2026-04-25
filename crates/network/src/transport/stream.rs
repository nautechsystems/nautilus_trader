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

//! Backend-agnostic WebSocket transport trait.

use std::pin::Pin;

use futures::{Sink, Stream};

use super::{error::TransportError, message::Message};

/// A backend-agnostic, bidirectional WebSocket transport.
///
/// This is the trait that the higher layers in `nautilus-network` (the
/// reconnecting client, the auth tracker, the subscription manager) consume.
/// Each transport backend implements it for its own native stream type.
///
/// The trait combines [`futures::Stream`] for incoming messages and
/// [`futures::Sink`] for outgoing messages, both keyed off the neutral
/// [`Message`] type and the neutral [`TransportError`].
pub trait WsTransport:
    Stream<Item = Result<Message, TransportError>>
    + Sink<Message, Error = TransportError>
    + Send
    + Unpin
{
}

impl<T> WsTransport for T where
    T: Stream<Item = Result<Message, TransportError>>
        + Sink<Message, Error = TransportError>
        + Send
        + Unpin
{
}

/// Boxed, dynamically-dispatched [`WsTransport`].
///
/// Used by the higher layers to hide the concrete backend stream type. The
/// per-backend `connect` functions return this type so callers don't need to
/// be generic over the backend.
pub type BoxedWsTransport = Pin<Box<dyn WsTransport>>;

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

//! Message senders for standalone clients and live runtime dispatch.

use std::thread::{self, ThreadId};

use super::dispatch::DispatchMessage;

/// A message sender which preserves callback ancestry when bound to a live runtime.
///
/// Sends on the runtime owner thread capture its active root. Sends on other threads
/// and sends without an active root are independent ingress. Conversion from a plain Tokio sender supports standalone
/// clients whose receivers consume domain events directly, without callback tracking.
#[derive(Debug)]
pub struct DispatchSender<T> {
    channel: DispatchChannel<T>,
}

/// A sender for data, execution, and system events.
pub type EventSender<T> = DispatchSender<T>;

impl<T> DispatchSender<T> {
    /// Binds a dispatch channel to the calling runtime thread.
    #[must_use]
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>) -> Self {
        Self {
            channel: DispatchChannel::Dispatch {
                sender,
                owner: thread::current().id(),
            },
        }
    }

    // panics-doc-ok
    /// Sends a message, preserving the active owner-thread callback root.
    ///
    /// # Errors
    ///
    /// Returns the undelivered envelope if the receiver is closed.
    ///
    /// # Panics
    ///
    /// Panics if the owner exhausts its channel context IDs.
    pub fn send(
        &self,
        message: T,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<DispatchMessage<T>>> {
        match &self.channel {
            DispatchChannel::Plain(sender) => sender
                .send(message)
                .map_err(|e| tokio::sync::mpsc::error::SendError(e.0.into())),
            DispatchChannel::Dispatch { sender, owner } => {
                sender.send(DispatchMessage::new(message, *owner))
            }
        }
    }

    /// Returns whether the receiver has closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        match &self.channel {
            DispatchChannel::Plain(sender) => sender.is_closed(),
            DispatchChannel::Dispatch { sender, .. } => sender.is_closed(),
        }
    }

    /// Returns whether both senders target the same channel.
    #[must_use]
    pub fn same_channel(&self, other: &Self) -> bool {
        match (&self.channel, &other.channel) {
            (DispatchChannel::Plain(left), DispatchChannel::Plain(right)) => {
                left.same_channel(right)
            }
            (
                DispatchChannel::Dispatch { sender: left, .. },
                DispatchChannel::Dispatch { sender: right, .. },
            ) => left.same_channel(right),
            _ => false,
        }
    }
}

impl<T> Clone for DispatchSender<T> {
    fn clone(&self) -> Self {
        let channel = match &self.channel {
            DispatchChannel::Plain(sender) => DispatchChannel::Plain(sender.clone()),
            DispatchChannel::Dispatch { sender, owner } => DispatchChannel::Dispatch {
                sender: sender.clone(),
                owner: *owner,
            },
        };

        Self { channel }
    }
}

impl<T> From<tokio::sync::mpsc::UnboundedSender<T>> for DispatchSender<T> {
    fn from(sender: tokio::sync::mpsc::UnboundedSender<T>) -> Self {
        Self {
            channel: DispatchChannel::Plain(sender),
        }
    }
}

#[derive(Debug)]
enum DispatchChannel<T> {
    Plain(tokio::sync::mpsc::UnboundedSender<T>),
    Dispatch {
        sender: tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>,
        owner: ThreadId,
    },
}

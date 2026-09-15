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

//! Callback context propagation for live command and event dispatch.

use std::thread::{self, ThreadId};

use crate::{
    actor::{ChainContext, SendChainContext, collect_command_contexts},
    runner::{TradingCommandMessage, dispatch_scoped_trading_command},
};

/// A live message carrying an owner-thread callback context through a send-safe channel.
///
/// Conversion with `From<T>` creates independent ingress without capturing a callback root.
/// External-thread sends and sends without an active root are also independent ingress.
/// Rooted messages must be processed on
/// their owner's thread. Foreign-thread destruction releases the root at the next owner
/// channel boundary, callback quiescence check, or dispatcher teardown.
#[derive(Debug)]
pub struct DispatchMessage<T> {
    message: Option<T>,
    context: Option<SendChainContext>,
}

impl<T> DispatchMessage<T> {
    /// Wraps a message, capturing callback ancestry only when called on `owner`.
    ///
    /// `owner` is the runtime thread that processes messages from this channel.
    ///
    /// # Panics
    ///
    /// Panics if the owner exhausts its channel context IDs.
    #[must_use]
    pub fn new(message: T, owner: ThreadId) -> Self {
        let context = (owner == thread::current().id())
            .then(SendChainContext::capture)
            .flatten();

        Self {
            message: Some(message),
            context,
        }
    }

    /// Processes the message under its originating callback context.
    ///
    /// # Panics
    ///
    /// Panics if a rooted message is processed outside its owner's thread.
    pub fn dispatch<R>(mut self, run: impl FnOnce(T) -> R) -> R {
        let context = self.take_context();
        context.with_chain(|| run(self.message.take().expect("message is present")))
    }

    /// Returns whether this message retains an originating callback root.
    #[must_use]
    pub fn is_rooted(&self) -> bool {
        self.context.is_some()
    }

    fn take_context(&self) -> ChainContext {
        collect_command_contexts();
        self.context
            .as_ref()
            .map_or_else(ChainContext::independent, |context| {
                context.take().expect("channel context is present")
            })
    }
}

impl DispatchMessage<TradingCommandMessage> {
    /// Dispatches the command and its deferred children under their captured callback roots.
    ///
    /// Calls `before` immediately before each endpoint dispatch, including child commands.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - A rooted message is processed outside its owner thread.
    /// - The observer or a command handler panics.
    pub fn dispatch_trading(mut self, before: impl FnMut(&TradingCommandMessage)) {
        let context = self.take_context();
        dispatch_scoped_trading_command(
            self.message.take().expect("message is present"),
            context,
            before,
        );
    }
}

impl<T: std::fmt::Display> std::fmt::Display for DispatchMessage<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.as_ref().expect("message is present").fmt(f)
    }
}

impl<T> From<T> for DispatchMessage<T> {
    fn from(message: T) -> Self {
        Self {
            message: Some(message),
            context: None,
        }
    }
}

impl<T> Drop for DispatchMessage<T> {
    fn drop(&mut self) {
        if self.message.is_none() {
            return;
        }

        let context = self
            .context
            .as_ref()
            .filter(|context| context.is_owner())
            .and_then(SendChainContext::take)
            .unwrap_or_else(ChainContext::independent);
        context.with_chain(|| drop(self.message.take()));
    }
}

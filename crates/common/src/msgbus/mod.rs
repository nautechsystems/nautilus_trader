// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! A common in-memory `MessageBus` supporting multiple messaging patterns:
//!
//! - Point-to-Point
//! - Pub/Sub
//! - Request/Response

pub mod core;
pub mod database;
pub mod handler;
pub mod listener;
pub mod matching;
pub mod message;
pub mod stubs;
pub mod switchboard;

#[cfg(test)]
mod tests;

pub use core::MessageBus;
use core::{Endpoint, Subscription};
use std::{
    self,
    any::Any,
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use handler::ShareableMessageHandler;
use matching::is_matching_backtracking;
use nautilus_core::UUID4;
use nautilus_model::data::Data;
use ustr::Ustr;

use crate::messages::data::DataResponse;
// Re-exports
pub use crate::msgbus::core::{MStr, Pattern, Topic};
pub use crate::msgbus::message::BusMessage;

// Thread-local storage for MessageBus instances. Each thread (including async runtimes)
// gets its own MessageBus instance, eliminating the need for unsafe Send/Sync implementations
// while maintaining the global singleton access pattern that the framework expects.
thread_local! {
    static MESSAGE_BUS: OnceCell<Rc<RefCell<MessageBus>>> = const { OnceCell::new() };
}

/// Sets the thread-local message bus.
///
/// # Panics
///
/// Panics if a message bus has already been set for this thread.
pub fn set_message_bus(msgbus: Rc<RefCell<MessageBus>>) {
    MESSAGE_BUS.with(|bus| {
        if bus.set(msgbus).is_err() {
            panic!("Failed to set MessageBus: already initialized for this thread");
        }
    });
}

/// Gets the thread-local message bus.
///
/// If no message bus has been set for this thread, a default one is created and initialized.
/// This ensures each thread gets its own MessageBus instance, preventing data races while
/// maintaining the singleton pattern that the codebase expects.
pub fn get_message_bus() -> Rc<RefCell<MessageBus>> {
    MESSAGE_BUS.with(|bus| {
        bus.get_or_init(|| {
            let msgbus = MessageBus::default();
            Rc::new(RefCell::new(msgbus))
        })
        .clone()
    })
}

/// Sends the `message` to the `endpoint`.
pub fn send_any(endpoint: MStr<Endpoint>, message: &dyn Any) {
    let handler = get_message_bus().borrow().get_endpoint(endpoint).cloned();
    if let Some(handler) = handler {
        handler.0.handle(message);
    } else {
        log::error!("send_any: no registered endpoint '{endpoint}'");
    }
}

/// Sends the `message` to the `endpoint`.
pub fn send<T: 'static>(endpoint: MStr<Endpoint>, message: T) {
    let handler = get_message_bus().borrow().get_endpoint(endpoint).cloned();
    if let Some(handler) = handler {
        handler.0.handle(&message);
    } else {
        log::error!("send: no registered endpoint '{endpoint}'");
    }
}

/// Sends the [`DataResponse`] to the registered correlation ID handler.
pub fn send_response(correlation_id: &UUID4, message: &DataResponse) {
    let handler = get_message_bus()
        .borrow()
        .get_response_handler(correlation_id)
        .cloned();

    if let Some(handler) = handler {
        handler.0.handle(message);
    } else {
        log::error!("send_response: handler not found for correlation_id '{correlation_id}'");
    }
}

/// Publish [`Data`] to a topic.
pub fn publish_data(topic: &Ustr, message: Data) {
    let matching_subs = get_message_bus().borrow_mut().matching_subscriptions(topic);

    for sub in matching_subs {
        sub.handler.0.handle(&message);
    }
}

/// Sends the response to the handler registered for the `correlation_id` (if found).
pub fn response(correlation_id: &UUID4, message: &dyn Any) {
    let handler = get_message_bus()
        .borrow()
        .get_response_handler(correlation_id)
        .cloned();
    if let Some(handler) = handler {
        handler.0.handle(message);
    } else {
        log::error!("response: handler not found for correlation_id '{correlation_id}'");
    }
}

pub fn register_response_handler(correlation_id: &UUID4, handler: ShareableMessageHandler) {
    if let Err(e) = get_message_bus()
        .borrow_mut()
        .register_response_handler(correlation_id, handler)
    {
        log::error!("Failed to register request handler: {e}");
    }
}

/// Publishes the `message` to the `topic`.
pub fn publish(topic: MStr<Topic>, message: &dyn Any) {
    log::trace!("Publishing topic '{topic}' {message:?}");
    let matching_subs = get_message_bus()
        .borrow_mut()
        .inner_matching_subscriptions(topic);

    log::trace!("Matched {} subscriptions", matching_subs.len());

    for sub in matching_subs {
        log::trace!("Matched {sub:?}");
        sub.handler.0.handle(message);
    }
}

/// Registers the `handler` for the `endpoint` address.
pub fn register(endpoint: MStr<Endpoint>, handler: ShareableMessageHandler) {
    log::debug!(
        "Registering endpoint '{endpoint}' with handler ID {}",
        handler.0.id(),
    );

    // Updates value if key already exists
    get_message_bus()
        .borrow_mut()
        .endpoints
        .insert(endpoint, handler);
}

/// Deregisters the handler for the `endpoint` address.
pub fn deregister(endpoint: MStr<Endpoint>) {
    log::debug!("Deregistering endpoint '{endpoint}'");

    // Removes entry if it exists for endpoint
    get_message_bus()
        .borrow_mut()
        .endpoints
        .shift_remove(&endpoint);
}

/// Subscribes the `handler` to the `pattern` with an optional `priority`.
///
/// # Warnings
///
/// Assigning priority handling is an advanced feature which *shouldn't
/// normally be needed by most users*. **Only assign a higher priority to the
/// subscription if you are certain of what you're doing**. If an inappropriate
/// priority is assigned then the handler may receive messages before core
/// system components have been able to process necessary calculations and
/// produce potential side effects for logically sound behavior.
pub fn subscribe(pattern: MStr<Pattern>, handler: ShareableMessageHandler, priority: Option<u8>) {
    let msgbus = get_message_bus();
    let mut msgbus_ref_mut = msgbus.borrow_mut();
    let sub = Subscription::new(pattern, handler, priority);

    log::debug!(
        "Subscribing {:?} for pattern '{}'",
        sub.handler,
        sub.pattern
    );

    // Prevent duplicate subscriptions for the exact pattern regardless of handler identity. This
    // guards against callers accidentally registering multiple handlers for the same topic, which
    // can lead to duplicated message delivery and unexpected side-effects.
    if msgbus_ref_mut.subscriptions.contains(&sub) {
        log::warn!("{sub:?} already exists");
        return;
    }

    // Find existing patterns which match this topic
    for (topic, subs) in msgbus_ref_mut.topics.iter_mut() {
        if is_matching_backtracking(*topic, sub.pattern) {
            // TODO: Consider binary_search and then insert
            subs.push(sub.clone());
            subs.sort();
            log::debug!("Added subscription for '{topic}'");
        }
    }

    msgbus_ref_mut.subscriptions.insert(sub);
}

pub fn subscribe_topic(topic: MStr<Topic>, handler: ShareableMessageHandler, priority: Option<u8>) {
    subscribe(topic.into(), handler, priority);
}

pub fn subscribe_str<T: AsRef<str>>(
    pattern: T,
    handler: ShareableMessageHandler,
    priority: Option<u8>,
) {
    subscribe(MStr::from(pattern), handler, priority);
}

/// Unsubscribes the `handler` from the `pattern`.
pub fn unsubscribe(pattern: MStr<Pattern>, handler: ShareableMessageHandler) {
    log::debug!("Unsubscribing {handler:?} from pattern '{pattern}'");

    let sub = core::Subscription::new(pattern, handler, None);

    get_message_bus()
        .borrow_mut()
        .topics
        .values_mut()
        .for_each(|subs| {
            if let Ok(index) = subs.binary_search(&sub) {
                subs.remove(index);
            }
        });

    let removed = get_message_bus().borrow_mut().subscriptions.remove(&sub);

    if removed {
        log::debug!("Handler for pattern '{pattern}' was removed");
    } else {
        log::debug!("No matching handler for pattern '{pattern}' was found");
    }
}

pub fn unsubscribe_topic(topic: MStr<Topic>, handler: ShareableMessageHandler) {
    unsubscribe(topic.into(), handler);
}

pub fn unsubscribe_str<T: AsRef<str>>(pattern: T, handler: ShareableMessageHandler) {
    unsubscribe(MStr::from(pattern), handler);
}

pub fn is_subscribed<T: AsRef<str>>(pattern: T, handler: ShareableMessageHandler) -> bool {
    let pattern = MStr::from(pattern.as_ref());
    let sub = Subscription::new(pattern, handler, None);
    get_message_bus().borrow().subscriptions.contains(&sub)
}

pub fn subscriptions_count<T: AsRef<str>>(topic: T) -> usize {
    get_message_bus().borrow().subscriptions_count(topic)
}

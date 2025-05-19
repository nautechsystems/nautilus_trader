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

pub mod database;
pub mod handler;
pub mod listener;
pub mod message;
pub mod stubs;
pub mod switchboard;

#[cfg(test)]
mod tests;

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    fmt::{self, Debug, Display},
    hash::{Hash, Hasher},
    ops::Deref,
    rc::Rc,
    sync::OnceLock,
};

use ahash::AHashMap;
use handler::ShareableMessageHandler;
use indexmap::IndexMap;
use nautilus_core::UUID4;
use nautilus_model::{data::Data, identifiers::TraderId};
use switchboard::MessagingSwitchboard;
use ustr::Ustr;

use crate::messages::data::DataResponse;
// Re-exports
pub use crate::msgbus::message::BusMessage;

#[derive(Debug)]
pub struct ShareableMessageBus(Rc<RefCell<MessageBus>>);

// SAFETY: Cannot be sent across thread boundaries
#[allow(unsafe_code)]
unsafe impl Send for ShareableMessageBus {}
#[allow(unsafe_code)]
unsafe impl Sync for ShareableMessageBus {}

static MESSAGE_BUS: OnceLock<ShareableMessageBus> = OnceLock::new();

/// Sets the global message bus.
///
/// # Panics
///
/// Panics if a message bus has already been set.
pub fn set_message_bus(msgbus: Rc<RefCell<MessageBus>>) {
    if MESSAGE_BUS.set(ShareableMessageBus(msgbus)).is_err() {
        panic!("Failed to set MessageBus");
    }
}

/// Gets the global message bus.
///
/// # Panics
///
/// Panics if the global message bus is uninitialized.
pub fn get_message_bus() -> Rc<RefCell<MessageBus>> {
    if MESSAGE_BUS.get().is_none() {
        // Initialize default message bus
        let msgbus = MessageBus::default();
        let msgbus = Rc::new(RefCell::new(msgbus));
        let _ = MESSAGE_BUS.set(ShareableMessageBus(msgbus.clone()));
        msgbus
    } else {
        MESSAGE_BUS.get().unwrap().0.clone()
    }
}

/// Sends the `message` to the `endpoint`.
pub fn send(endpoint: &Ustr, message: &dyn Any) {
    let handler = get_message_bus().borrow().get_endpoint(endpoint).cloned();
    if let Some(handler) = handler {
        handler.0.handle(message);
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
        log::error!(
            "Failed to handle response: handler not found for correlation_id {correlation_id}"
        )
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
pub fn publish(topic: &Ustr, message: &dyn Any) {
    log::trace!("Publishing topic '{topic}' {message:?}");
    let matching_subs = get_message_bus().borrow_mut().matching_subscriptions(topic);

    log::trace!("Matched {} subscriptions", matching_subs.len());

    for sub in matching_subs {
        log::trace!("Matched {sub:?}");
        sub.handler.0.handle(message);
    }
}

/// Registers the `handler` for the `endpoint` address.
pub fn register<T: AsRef<str>>(endpoint: T, handler: ShareableMessageHandler) {
    let endpoint = Ustr::from(endpoint.as_ref());

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
pub fn deregister<T: AsRef<str>>(endpoint: T) {
    let endpoint = Ustr::from(endpoint.as_ref());

    log::debug!("Deregistering endpoint '{endpoint}'");

    // Removes entry if it exists for endpoint
    get_message_bus()
        .borrow_mut()
        .endpoints
        .shift_remove(&endpoint);
}

/// Subscribes the given `handler` to the `pattern` with an optional `priority`.
///
/// # Warnings
///
/// Assigning priority handling is an advanced feature which *shouldn't
/// normally be needed by most users*. **Only assign a higher priority to the
/// subscription if you are certain of what you're doing**. If an inappropriate
/// priority is assigned then the handler may receive messages before core
/// system components have been able to process necessary calculations and
/// produce potential side effects for logically sound behavior.
pub fn subscribe<T: AsRef<str>>(
    pattern: T,
    handler: ShareableMessageHandler,
    priority: Option<u8>,
) {
    let pattern = Pattern::from(pattern);
    inner_subscribe(pattern, handler, priority);
}

fn inner_subscribe(pattern: Pattern, handler: ShareableMessageHandler, priority: Option<u8>) {
    let msgbus = get_message_bus();
    let mut msgbus_ref_mut = msgbus.borrow_mut();
    let sub = Subscription::new(pattern, handler, priority);

    log::debug!(
        "Subscribing {:?} for pattern '{}'",
        sub.handler,
        sub.pattern
    );

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

/// Unsubscribes the `handler` from the `pattern`.
pub fn unsubscribe<T: AsRef<str>>(pattern: T, handler: ShareableMessageHandler) {
    let pattern = Pattern::from(pattern);
    inner_unsubscribe(pattern, handler);
}

fn inner_unsubscribe(pattern: Pattern, handler: ShareableMessageHandler) {
    log::debug!("Unsubscribing {handler:?} from pattern '{pattern}'");

    let sub = Subscription::new(pattern, handler, None);

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

pub fn is_subscribed<T: AsRef<str>>(pattern: T, handler: ShareableMessageHandler) -> bool {
    let pattern = Pattern::from(pattern);
    let sub = Subscription::new(pattern, handler, None);
    get_message_bus().borrow().subscriptions.contains(&sub)
}

pub fn subscriptions_count<T: AsRef<str>>(topic: T) -> usize {
    get_message_bus().borrow().subscriptions_count(topic)
}

/// A string pattern for a subscription. The pattern is used to match topics.
///
/// A pattern is made of characters:
/// - `*` - match 0 or more characters
/// - `?` - match any character once
/// - `a-z` - match the specific character
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pattern(pub Ustr);

impl<T: AsRef<str>> From<T> for Pattern {
    fn from(value: T) -> Self {
        Self(Ustr::from(value.as_ref()))
    }
}

impl Deref for Pattern {
    type Target = Ustr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Topic> for Pattern {
    fn from(value: Topic) -> Self {
        Self(value.0)
    }
}

/// A string topic for publishing data. It is a fully qualified pattern i.e.
/// wildcard characters are not allowed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Topic(pub Ustr);

impl Topic {
    pub fn validate<T: AsRef<str>>(topic: T) -> bool {
        let topic = Ustr::from(topic.as_ref());
        !topic.chars().any(|c| c == '*' || c == '?')
    }
}

impl<T: AsRef<str>> From<T> for Topic {
    fn from(value: T) -> Self {
        Self(Ustr::from(value.as_ref()))
    }
}

impl Deref for Topic {
    type Target = Ustr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a subscription to a particular topic.
///
/// This is an internal class intended to be used by the message bus to organize
/// topics and their subscribers.
///

#[derive(Clone, Debug)]
pub struct Subscription {
    /// The shareable message handler for the subscription.
    pub handler: ShareableMessageHandler,
    /// Store a copy of the handler ID for faster equality checks.
    pub handler_id: Ustr,
    /// The pattern for the subscription.
    pub pattern: Pattern,
    /// The priority for the subscription determines the ordering of handlers receiving
    /// messages being processed, higher priority handlers will receive messages before
    /// lower priority handlers.
    pub priority: u8,
}

impl Subscription {
    /// Creates a new [`Subscription`] instance.
    #[must_use]
    pub fn new(pattern: Pattern, handler: ShareableMessageHandler, priority: Option<u8>) -> Self {
        Self {
            handler_id: handler.0.id(),
            pattern,
            handler,
            priority: priority.unwrap_or(0),
        }
    }
}

impl PartialEq<Self> for Subscription {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern && self.handler_id == other.handler_id
    }
}

impl Eq for Subscription {}

impl PartialOrd for Subscription {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Subscription {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.priority.cmp(&self.priority)
    }
}

impl Hash for Subscription {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pattern.hash(state);
        self.handler_id.hash(state);
    }
}

/// A generic message bus to facilitate various messaging patterns.
///
/// The bus provides both a producer and consumer API for Pub/Sub, Req/Rep, as
/// well as direct point-to-point messaging to registered endpoints.
///
/// Pub/Sub wildcard patterns for hierarchical topics are possible:
///  - `*` asterisk represents one or more characters in a pattern.
///  - `?` question mark represents a single character in a pattern.
///
/// Given a topic and pattern potentially containing wildcard characters, i.e.
/// `*` and `?`, where `?` can match any single character in the topic, and `*`
/// can match any number of characters including zero characters.
///
/// The asterisk in a wildcard matches any character zero or more times. For
/// example, `comp*` matches anything beginning with `comp` which means `comp`,
/// `complete`, and `computer` are all matched.
///
/// A question mark matches a single character once. For example, `c?mp` matches
/// `camp` and `comp`. The question mark can also be used more than once.
/// For example, `c??p` would match both of the above examples and `coop`.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug)]
pub struct MessageBus {
    /// The trader ID associated with the message bus.
    pub trader_id: TraderId,
    /// The instance ID associated with the message bus.
    pub instance_id: UUID4,
    /// The name for the message bus.
    pub name: String,
    /// If the message bus is backed by a database.
    pub has_backing: bool,
    /// The switchboard for built-in endpoints.
    pub switchboard: MessagingSwitchboard,
    /// Active subscriptions.
    subscriptions: BTreeSet<Subscription>,
    /// Maps a topic to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    topics: IndexMap<Topic, Vec<Subscription>>,
    /// Index of endpoint addresses and their handlers.
    endpoints: IndexMap<Ustr, ShareableMessageHandler>,
    /// Index of request correlation IDs and their response handlers.
    correlation_index: AHashMap<UUID4, ShareableMessageHandler>,
}

// SAFETY: Message bus is not meant to be passed between threads
#[allow(unsafe_code)]
unsafe impl Send for MessageBus {}
#[allow(unsafe_code)]
unsafe impl Sync for MessageBus {}

impl MessageBus {
    /// Creates a new [`MessageBus`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        name: Option<String>,
        _config: Option<HashMap<String, serde_json::Value>>,
    ) -> Self {
        Self {
            trader_id,
            instance_id,
            name: name.unwrap_or(stringify!(MessageBus).to_owned()),
            switchboard: MessagingSwitchboard::default(),
            subscriptions: BTreeSet::new(),
            topics: IndexMap::new(),
            endpoints: IndexMap::new(),
            correlation_index: AHashMap::new(),
            has_backing: false,
        }
    }

    /// Returns the message bus instances memory address.
    #[must_use]
    pub fn memory_address(&self) -> String {
        format!("{:?}", std::ptr::from_ref(self))
    }

    /// Returns the registered endpoint addresses.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&str> {
        self.endpoints.keys().map(Ustr::as_str).collect()
    }

    /// Returns actively subscribed patterns.
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.pattern.as_str())
            .collect()
    }

    /// Returns whether there are subscribers for the given `topic`.
    pub fn has_subscribers<T: AsRef<str>>(&self, topic: T) -> bool {
        self.subscriptions_count(topic) > 0
    }

    /// Returns the count of subscribers for the given `topic`.
    #[must_use]
    pub fn subscriptions_count<T: AsRef<str>>(&self, topic: T) -> usize {
        let topic = Topic::from(topic);
        self.topics
            .get(&topic)
            .map(|subs| subs.len())
            .unwrap_or_else(|| self.find_topic_matches(topic).len())
    }

    /// Returns active subscriptions.
    #[must_use]
    pub fn subscriptions(&self) -> Vec<&Subscription> {
        self.subscriptions.iter().collect()
    }

    /// Returns the handler IDs for actively subscribed patterns.
    #[must_use]
    pub fn subscription_handler_ids(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.handler_id.as_str())
            .collect()
    }

    /// Returns whether there is a registered endpoint for the given `pattern`.
    #[must_use]
    pub fn is_registered<T: AsRef<str>>(&self, endpoint: T) -> bool {
        self.endpoints.contains_key(&Ustr::from(endpoint.as_ref()))
    }

    /// Returns whether the given `handler` is subscribed to the given `pattern`.
    #[must_use]
    pub fn is_subscribed<T: AsRef<str>>(
        &self,
        pattern: T,
        handler: ShareableMessageHandler,
    ) -> bool {
        let pattern = Pattern::from(pattern);
        let sub = Subscription::new(pattern, handler, None);
        self.subscriptions.contains(&sub)
    }

    /// Close the message bus which will close the sender channel and join the thread.
    ///
    /// # Errors
    ///
    /// This function never returns an error (TBD).
    pub const fn close(&self) -> anyhow::Result<()> {
        // TODO: Integrate the backing database
        Ok(())
    }

    /// Returns the handler for the given `endpoint`.
    #[must_use]
    pub fn get_endpoint<T: AsRef<str>>(&self, endpoint: T) -> Option<&ShareableMessageHandler> {
        self.endpoints.get(&Ustr::from(endpoint.as_ref()))
    }

    /// Returns the handler for the given `correlation_id`.
    #[must_use]
    pub fn get_response_handler(&self, correlation_id: &UUID4) -> Option<&ShareableMessageHandler> {
        self.correlation_index.get(correlation_id)
    }

    /// Finds the subscriptions with pattern matching the given `topic`.
    fn find_topic_matches(&self, topic: Topic) -> Vec<Subscription> {
        self.subscriptions
            .iter()
            .filter_map(|sub| {
                if is_matching_backtracking(topic, sub.pattern) {
                    Some(sub.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    #[must_use]
    /// Finds the subscriptions which match the given `topic` and caches the
    /// results in the `patterns` map.
    pub fn matching_subscriptions<T: AsRef<str>>(&mut self, topic: T) -> Vec<Subscription> {
        let topic = Topic::from(topic);
        self.inner_matching_subscriptions(topic)
    }

    fn inner_matching_subscriptions(&mut self, topic: Topic) -> Vec<Subscription> {
        self.topics.get(&topic).cloned().unwrap_or_else(|| {
            let mut matches = self.find_topic_matches(topic);
            matches.sort();
            self.topics.insert(topic, matches.clone());
            matches
        })
    }

    /// Register a response handler for a specific correlation ID.
    ///
    /// # Errors
    ///
    /// Returns an error if a handler is already registered for the given correlation ID.
    pub fn register_response_handler(
        &mut self,
        correlation_id: &UUID4,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<()> {
        if self.correlation_index.contains_key(correlation_id) {
            return Err(anyhow::anyhow!(
                "Correlation ID <{correlation_id}> already has a registered handler",
            ));
        }

        self.correlation_index.insert(*correlation_id, handler);

        Ok(())
    }
}

/// Data specific functions.
impl MessageBus {
    // /// Send a [`DataRequest`] to an endpoint that must be a data client implementation.
    // pub fn send_data_request(&self, message: DataRequest) {
    //     // TODO: log error
    //     if let Some(client) = self.get_client(&message.client_id, message.venue) {
    //         let _ = client.request(message);
    //     }
    // }
    //
    // /// Send a [`SubscriptionCommand`] to an endpoint that must be a data client implementation.
    // pub fn send_subscription_command(&self, message: SubscriptionCommand) {
    //     if let Some(client) = self.get_client(&message.client_id, message.venue) {
    //         client.through_execute(message);
    //     }
    // }

    /// Register message bus globally
    pub fn register_message_bus(self) -> Rc<RefCell<MessageBus>> {
        let msgbus = Rc::new(RefCell::new(self));
        set_message_bus(msgbus.clone());
        msgbus
    }
}

/// Match a topic and a string pattern
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
#[must_use]
pub fn is_matching(topic: &Ustr, pattern: &Ustr) -> bool {
    let mut table = [[false; 256]; 256];
    table[0][0] = true;

    let m = pattern.len();
    let n = topic.len();

    pattern.chars().enumerate().for_each(|(j, c)| {
        if c == '*' {
            table[0][j + 1] = table[0][j];
        }
    });

    topic.chars().enumerate().for_each(|(i, tc)| {
        pattern.chars().enumerate().for_each(|(j, pc)| {
            if pc == '*' {
                table[i + 1][j + 1] = table[i][j + 1] || table[i + 1][j];
            } else if pc == '?' || tc == pc {
                table[i + 1][j + 1] = table[i][j];
            }
        });
    });

    table[n][m]
}

/// Match a topic and a string pattern using iterative backtracking algorithm
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
pub fn is_matching_backtracking(topic: Topic, pattern: Pattern) -> bool {
    let topic_bytes = topic.as_bytes();
    let pattern_bytes = pattern.as_bytes();

    is_matching_fast(topic_bytes, pattern_bytes)
}

#[must_use]
pub fn is_matching_fast(topic: &[u8], pattern: &[u8]) -> bool {
    // Stack to store states for backtracking (topic_idx, pattern_idx)
    let mut stack = vec![(0, 0)];

    while let Some((mut i, mut j)) = stack.pop() {
        loop {
            // Found a match if we've consumed both strings
            if i == topic.len() && j == pattern.len() {
                return true;
            }

            // If we've reached the end of the pattern, break to try other paths
            if j == pattern.len() {
                break;
            }

            // Handle '*' wildcard
            if pattern[j] == b'*' {
                // Try skipping '*' entirely first
                stack.push((i, j + 1));

                // Continue with matching current character and keeping '*'
                if i < topic.len() {
                    i += 1;
                    continue;
                }
                break;
            }
            // Handle '?' or exact character match
            else if i < topic.len() && (pattern[j] == b'?' || topic[i] == pattern[j]) {
                // Continue matching linearly without stack operations
                i += 1;
                j += 1;
                continue;
            }

            // No match found in current path
            break;
        }
    }

    false
}

impl Default for MessageBus {
    /// Creates a new default [`MessageBus`] instance.
    fn default() -> Self {
        Self::new(TraderId::from("TRADER-001"), UUID4::new(), None, None)
    }
}

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

//! A common in-memory `MessageBus` for loosely coupled message passing patterns.

pub mod database;
pub mod handler;
pub mod listener;
pub mod stubs;
pub mod switchboard;

use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    rc::Rc,
    sync::OnceLock,
};

use bytes::Bytes;
use handler::ShareableMessageHandler;
use indexmap::IndexMap;
use nautilus_core::UUID4;
use nautilus_model::{data::Data, identifiers::TraderId};
use serde::{Deserialize, Serialize};
use switchboard::MessagingSwitchboard;
use ustr::Ustr;

use crate::messages::data::DataResponse;

pub const CLOSE_TOPIC: &str = "CLOSE";

/// Represents a bus message including a topic and payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct BusMessage {
    /// The topic to publish on.
    pub topic: String,
    /// The serialized payload for the message.
    pub payload: Bytes,
}

impl Display for BusMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}",
            self.topic,
            String::from_utf8_lossy(&self.payload)
        )
    }
}

pub struct MessageBusWrapper(Rc<RefCell<MessageBus>>);

unsafe impl Send for MessageBusWrapper {}
unsafe impl Sync for MessageBusWrapper {}

static MESSAGE_BUS: OnceLock<MessageBusWrapper> = OnceLock::new();

pub fn set_message_bus(msgbus: Rc<RefCell<MessageBus>>) {
    if MESSAGE_BUS.set(MessageBusWrapper(msgbus)).is_err() {
        panic!("Failed to set MessageBus");
    }
}

pub fn get_message_bus() -> Rc<RefCell<MessageBus>> {
    if MESSAGE_BUS.get().is_none() {
        // Initialize default message bus
        let msgbus = MessageBus::default();
        let msgbus = Rc::new(RefCell::new(msgbus));
        let _ = MESSAGE_BUS.set(MessageBusWrapper(msgbus.clone()));
        msgbus
    } else {
        MESSAGE_BUS.get().unwrap().0.clone()
    }
}

pub fn send(endpoint: &Ustr, message: &dyn Any) {
    let handler = get_message_bus().borrow().get_endpoint(endpoint).cloned();
    if let Some(handler) = handler {
        handler.0.handle(message);
    }
}

/// Publish a message to a topic.
pub fn publish(topic: &Ustr, message: &dyn Any) {
    log::trace!(
        "Publishing topic '{topic}' {message:?} at {}",
        get_message_bus().borrow().memory_address()
    );
    let matching_subs = get_message_bus().borrow().matching_subscriptions(topic);

    log::trace!("Matched {} subscriptions", matching_subs.len());

    for sub in matching_subs {
        log::trace!("Matched {sub:?}");
        sub.handler.0.handle(message);
    }
}

/// Registers the given `handler` for the `endpoint` address.
pub fn register<T: AsRef<str>>(endpoint: T, handler: ShareableMessageHandler) {
    log::debug!(
        "Registering endpoint '{}' with handler ID {} at {}",
        endpoint.as_ref(),
        handler.0.id(),
        get_message_bus().borrow().memory_address(),
    );

    // Updates value if key already exists
    get_message_bus()
        .borrow_mut()
        .endpoints
        .insert(Ustr::from(endpoint.as_ref()), handler);
}

/// Deregisters the given `handler` for the `endpoint` address.
pub fn deregister(endpoint: &Ustr) {
    log::debug!(
        "Deregistering endpoint '{endpoint}' at {}",
        get_message_bus().borrow().memory_address()
    );
    // Removes entry if it exists for endpoint
    get_message_bus()
        .borrow_mut()
        .endpoints
        .shift_remove(endpoint);
}

/// Subscribes the given `handler` to the `topic`.
pub fn subscribe<T: AsRef<str>>(topic: T, handler: ShareableMessageHandler, priority: Option<u8>) {
    log::debug!(
        "Subscribing for topic '{}' at {}",
        topic.as_ref(),
        get_message_bus().borrow().memory_address(),
    );

    let msgbus = get_message_bus();
    let mut msgbus_ref_mut = msgbus.borrow_mut();

    let sub = Subscription::new(topic.as_ref(), handler, priority);
    if msgbus_ref_mut.subscriptions.contains_key(&sub) {
        log::error!("{sub:?} already exists");
        return;
    }

    // Find existing patterns which match this topic
    let mut matches = Vec::new();
    for (pattern, subs) in msgbus_ref_mut.patterns.iter_mut() {
        if is_matching(&Ustr::from(topic.as_ref()), pattern) {
            subs.push(sub.clone());
            subs.sort();
            // subs.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.cmp(b)));
            matches.push(*pattern);
        }
    }

    matches.sort();

    msgbus_ref_mut.subscriptions.insert(sub, matches);
}

/// Unsubscribes the given `handler` from the `topic`.
pub fn unsubscribe<T: AsRef<str>>(topic: T, handler: ShareableMessageHandler) {
    log::debug!(
        "Unsubscribing for topic '{}' at {}",
        topic.as_ref(),
        get_message_bus().borrow().memory_address(),
    );
    let sub = Subscription::new(topic, handler, None);
    get_message_bus()
        .borrow_mut()
        .subscriptions
        .shift_remove(&sub);
}

pub fn is_subscribed<T: AsRef<str>>(topic: T, handler: ShareableMessageHandler) -> bool {
    let sub = Subscription::new(topic, handler, None);
    get_message_bus().borrow().subscriptions.contains_key(&sub)
}

pub fn subscriptions_count<T: AsRef<str>>(topic: T) -> usize {
    get_message_bus().borrow().subscriptions_count(topic)
}

/// Represents a subscription to a particular topic.
///
/// This is an internal class intended to be used by the message bus to organize
/// topics and their subscribers.
///
/// # Warnings
///
/// Assigning priority handling is an advanced feature which *shouldn't
/// normally be needed by most users*. **Only assign a higher priority to the
/// subscription if you are certain of what you're doing**. If an inappropriate
/// priority is assigned then the handler may receive messages before core
/// system components have been able to process necessary calculations and
/// produce potential side effects for logically sound behavior.
#[derive(Clone)]
pub struct Subscription {
    /// The shareable message handler for the subscription.
    pub handler: ShareableMessageHandler,
    /// Store a copy of the handler ID for faster equality checks.
    pub handler_id: Ustr,
    /// The topic for the subscription.
    pub topic: Ustr,
    /// The priority for the subscription determines the ordering of handlers receiving
    /// messages being processed, higher priority handlers will receive messages before
    /// lower priority handlers.
    pub priority: u8,
}

impl Subscription {
    /// Creates a new [`Subscription`] instance.
    #[must_use]
    pub fn new<T: AsRef<str>>(
        topic: T,
        handler: ShareableMessageHandler,
        priority: Option<u8>,
    ) -> Self {
        let handler_id = handler.0.id();

        Self {
            handler_id,
            topic: Ustr::from(topic.as_ref()),
            handler,
            priority: priority.unwrap_or(0),
        }
    }
}

impl Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Subscription {{ topic: {}, handler: {}, priority: {} }}",
            self.topic, self.handler_id, self.priority
        )
    }
}

impl PartialEq<Self> for Subscription {
    fn eq(&self, other: &Self) -> bool {
        self.topic == other.topic && self.handler_id == other.handler_id
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
        self.topic.hash(state);
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
    /// Mapping from topic to the corresponding handler
    /// a topic can be a string with wildcards
    /// * '?' - any character
    /// * '*' - any number of any characters
    subscriptions: IndexMap<Subscription, Vec<Ustr>>,
    /// Maps a pattern to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    patterns: IndexMap<Ustr, Vec<Subscription>>,
    /// Handles a message or a request destined for a specific endpoint.
    endpoints: IndexMap<Ustr, ShareableMessageHandler>,
}

// SAFETY: Message bus is not meant to be passed between threads
unsafe impl Send for MessageBus {}
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
            subscriptions: IndexMap::new(),
            patterns: IndexMap::new(),
            endpoints: IndexMap::new(),
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

    /// Returns the topics for active subscriptions.
    #[must_use]
    pub fn topics(&self) -> Vec<&str> {
        self.subscriptions
            .keys()
            .map(|s| s.topic.as_str())
            .collect()
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn has_subscribers<T: AsRef<str>>(&self, pattern: T) -> bool {
        self.matching_handlers(&Ustr::from(pattern.as_ref()))
            .next()
            .is_some()
    }

    /// Returns the count of subscribers for the given `pattern`.
    #[must_use]
    pub fn subscriptions_count<T: AsRef<str>>(&self, pattern: T) -> usize {
        self.matching_subscriptions(&Ustr::from(pattern.as_ref()))
            .len()
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn subscriptions(&self) -> Vec<&Subscription> {
        self.subscriptions.keys().collect()
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn subscription_handler_ids(&self) -> Vec<&str> {
        self.subscriptions
            .keys()
            .map(|s| s.handler_id.as_str())
            .collect()
    }

    /// Returns whether there is a registered endpoint for the given `pattern`.
    #[must_use]
    pub fn is_registered<T: AsRef<str>>(&self, endpoint: T) -> bool {
        self.endpoints.contains_key(&Ustr::from(endpoint.as_ref()))
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn is_subscribed<T: AsRef<str>>(&self, topic: T, handler: ShareableMessageHandler) -> bool {
        let sub = Subscription::new(topic, handler, None);
        self.subscriptions.contains_key(&sub)
    }

    /// Close the message bus which will close the sender channel and join the thread.
    pub const fn close(&self) -> anyhow::Result<()> {
        // TODO: Integrate the backing database
        Ok(())
    }
    /// Returns the handler for the given `endpoint`.
    #[must_use]
    pub fn get_endpoint<T: AsRef<str>>(&self, endpoint: T) -> Option<&ShareableMessageHandler> {
        self.endpoints.get(&Ustr::from(endpoint.as_ref()))
    }

    #[must_use]
    pub fn matching_subscriptions(&self, pattern: &Ustr) -> Vec<Subscription> {
        let mut matching_subs: Vec<Subscription> = Vec::new();

        // Collect matching subscriptions from direct subscriptions
        matching_subs.extend(self.subscriptions.iter().filter_map(|(sub, _)| {
            if is_matching(&sub.topic, pattern) {
                Some(sub.clone())
            } else {
                None
            }
        }));

        // Collect matching subscriptions from pattern-based subscriptions
        // TODO: Improve efficiency of this
        for subs in self.patterns.values() {
            let filtered_subs: Vec<Subscription> = subs.to_vec();

            matching_subs.extend(filtered_subs);
        }

        // Sort into priority order
        matching_subs.sort();
        matching_subs
    }

    fn matching_handlers<'a>(
        &'a self,
        pattern: &'a Ustr,
    ) -> impl Iterator<Item = &'a ShareableMessageHandler> {
        self.subscriptions.iter().filter_map(move |(sub, _)| {
            if is_matching(&sub.topic, pattern) {
                Some(&sub.handler)
            } else {
                None
            }
        })
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

    /// Send a [`DataResponse`] to an endpoint that must be an actor.
    pub fn send_response(&self, message: DataResponse) {
        if let Some(handler) = self.get_endpoint(message.client_id.inner()) {
            handler.0.handle(&message);
        }
    }

    /// Publish [`Data`] to a topic.
    pub fn publish_data(&self, topic: &Ustr, message: Data) {
        let matching_subs = self.matching_subscriptions(topic);

        for sub in matching_subs {
            sub.handler.0.handle(&message);
        }
    }

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

impl Default for MessageBus {
    /// Creates a new default [`MessageBus`] instance.
    fn default() -> Self {
        Self::new(TraderId::from("TRADER-001"), UUID4::new(), None, None)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub(crate) mod tests {

    use nautilus_core::UUID4;
    use rstest::*;
    use stubs::check_handler_was_called;

    use super::*;
    use crate::msgbus::stubs::{get_call_check_shareable_handler, get_stub_shareable_handler};

    #[rstest]
    fn test_new() {
        let trader_id = TraderId::from("trader-001");
        let msgbus = MessageBus::new(trader_id, UUID4::new(), None, None);

        assert_eq!(msgbus.trader_id, trader_id);
        assert_eq!(msgbus.name, stringify!(MessageBus));
    }

    #[rstest]
    fn test_endpoints_when_no_endpoints() {
        let msgbus = get_message_bus();
        assert!(msgbus.borrow().endpoints().is_empty());
    }

    #[rstest]
    fn test_topics_when_no_subscriptions() {
        let msgbus = get_message_bus();
        assert!(msgbus.borrow().topics().is_empty());
        assert!(!msgbus.borrow().has_subscribers("my-topic"));
    }

    #[rstest]
    fn test_is_subscribed_when_no_subscriptions() {
        let msgbus = get_message_bus();
        let handler = get_stub_shareable_handler(None);

        assert!(!msgbus.borrow().is_subscribed("my-topic", handler));
    }

    #[rstest]
    fn test_is_registered_when_no_registrations() {
        let msgbus = get_message_bus();
        assert!(!msgbus.borrow().is_registered("MyEndpoint"));
    }

    #[rstest]
    fn test_regsiter_endpoint() {
        let msgbus = get_message_bus();
        let endpoint = "MyEndpoint";
        let handler = get_stub_shareable_handler(None);

        register(endpoint, handler);

        assert_eq!(msgbus.borrow().endpoints(), vec![endpoint.to_string()]);
        assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
    }

    #[rstest]
    fn test_endpoint_send() {
        let msgbus = get_message_bus();
        let endpoint = Ustr::from("MyEndpoint");
        let handler = get_call_check_shareable_handler(None);

        register(endpoint, handler.clone());
        assert!(msgbus.borrow().get_endpoint(endpoint).is_some());
        assert!(!check_handler_was_called(handler.clone()));

        // Send a message to the endpoint
        send(&endpoint, &"Test Message");
        assert!(check_handler_was_called(handler));
    }

    #[rstest]
    fn test_deregsiter_endpoint() {
        let msgbus = get_message_bus();
        let endpoint = Ustr::from("MyEndpoint");
        let handler = get_stub_shareable_handler(None);

        register(endpoint, handler);
        deregister(&endpoint);

        assert!(msgbus.borrow().endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let msgbus = get_message_bus();
        let topic = "my-topic";
        let handler = get_stub_shareable_handler(None);

        subscribe(topic, handler, Some(1));

        assert!(msgbus.borrow().has_subscribers(topic));
        assert_eq!(msgbus.borrow().topics(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let msgbus = get_message_bus();
        let topic = "my-topic";
        let handler = get_stub_shareable_handler(None);

        subscribe(topic, handler.clone(), None);
        unsubscribe(topic, handler);

        assert!(!msgbus.borrow().has_subscribers(topic));
        assert!(msgbus.borrow().topics().is_empty());
    }

    #[rstest]
    fn test_matching_subscriptions() {
        let msgbus = get_message_bus();
        let topic = "my-topic";

        let handler_id1 = Ustr::from("1");
        let handler1 = get_stub_shareable_handler(Some(handler_id1));

        let handler_id2 = Ustr::from("2");
        let handler2 = get_stub_shareable_handler(Some(handler_id2));

        let handler_id3 = Ustr::from("3");
        let handler3 = get_stub_shareable_handler(Some(handler_id3));

        let handler_id4 = Ustr::from("4");
        let handler4 = get_stub_shareable_handler(Some(handler_id4));

        subscribe(topic, handler1, None);
        subscribe(topic, handler2, None);
        subscribe(topic, handler3, Some(1));
        subscribe(topic, handler4, Some(2));
        let topic = Ustr::from(topic);

        let subs = msgbus.borrow().matching_subscriptions(&topic);
        assert_eq!(subs.len(), 4);
        assert_eq!(subs[0].handler_id, handler_id4);
        assert_eq!(subs[1].handler_id, handler_id3);
        assert_eq!(subs[2].handler_id, handler_id1);
        assert_eq!(subs[3].handler_id, handler_id2);
    }

    #[rstest]
    #[case("*", "*", true)]
    #[case("a", "*", true)]
    #[case("a", "a", true)]
    #[case("a", "b", false)]
    #[case("data.quotes.BINANCE", "data.*", true)]
    #[case("data.quotes.BINANCE", "data.quotes*", true)]
    #[case("data.quotes.BINANCE", "data.*.BINANCE", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.*", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH*", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH???", false)]
    #[case("data.trades.BINANCE.ETHUSD", "data.*.BINANCE.ETH???", true)]
    // We don't support [seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[HC]USDT", false)]
    // We don't support [!seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[!ABC]USDT", false)]
    // We don't support [^seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[^ABC]USDT", false)]
    fn test_is_matching(#[case] topic: &str, #[case] pattern: &str, #[case] expected: bool) {
        assert_eq!(
            is_matching(&Ustr::from(topic), &Ustr::from(pattern)),
            expected
        );
    }
}

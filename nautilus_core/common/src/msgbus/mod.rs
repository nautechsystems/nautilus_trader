// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
pub mod stubs;

use std::{
    any::Any,
    collections::HashMap,
    fmt::Debug,
    hash::{Hash, Hasher},
    rc::Rc,
};

use indexmap::IndexMap;
use log::error;
use nautilus_core::uuid::UUID4;
use nautilus_model::{
    data::Data,
    identifiers::{ClientId, TraderId, Venue},
};
use ustr::Ustr;

use crate::{
    client::DataClientAdapter,
    messages::data::{DataRequest, DataResponse, SubscriptionCommand},
};

pub const CLOSE_TOPIC: &str = "CLOSE";

pub trait MessageHandler: Any {
    fn id(&self) -> Ustr;
    fn handle(&self, message: &dyn Any);
    fn handle_response(&self, resp: DataResponse);
    fn handle_data(&self, resp: &Data);
    fn as_any(&self) -> &dyn Any;
}

#[derive(Clone)]
#[repr(transparent)]
pub struct ShareableMessageHandler(pub Rc<dyn MessageHandler>);

impl From<Rc<dyn MessageHandler>> for ShareableMessageHandler {
    fn from(value: Rc<dyn MessageHandler>) -> Self {
        Self(value)
    }
}

// Message handlers are not expected to be sent across thread boundaries
unsafe impl Send for ShareableMessageHandler {}

// Represents a subscription to a particular topic.
//
// This is an internal class intended to be used by the message bus to organize
// topics and their subscribers.
#[derive(Clone)]
pub struct Subscription {
    pub handler: ShareableMessageHandler,
    /// Store a copy of the handler ID for faster equality checks.
    pub handler_id: Ustr,
    pub topic: Ustr,
    /// The priority for the subscription determines the ordering of
    /// handlers receiving messages being processed, higher priority
    /// handlers will receive messages before lower priority handlers.
    ///
    /// # Warnings
    ///
    /// Assigning priority handling is an advanced feature which *shouldn't
    /// normally be needed by most users*. **Only assign a higher priority to the
    /// subscription if you are certain of what you're doing**. If an inappropriate
    /// priority is assigned then the handler may receive messages before core
    /// system components have been able to process necessary calculations and
    /// produce potential side effects for logically sound behavior.
    pub priority: u8,
}

impl Subscription {
    /// Creates a new [`Subscription`] instance.
    #[must_use]
    pub fn new(topic: Ustr, handler: ShareableMessageHandler, priority: Option<u8>) -> Self {
        let handler_id = handler.0.id();

        Self {
            handler_id,
            topic,
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
#[allow(clippy::type_complexity)] // Complexity will reduce when Cython eliminated
pub struct MessageBus {
    /// The trader ID associated with the message bus.
    pub trader_id: TraderId,
    /// The instance ID associated with the message bus.
    pub instance_id: UUID4,
    /// The name for the message bus.
    pub name: String,
    /// If the message bus is backed by a database.
    pub has_backing: bool,
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
    /// Handles data and subscriptions requests for a specific data client
    pub clients: IndexMap<ClientId, DataClientAdapter>,
    routing_map: HashMap<Venue, ClientId>,
}

/// Message bus is not meant to be passed between threads
unsafe impl Send for MessageBus {}

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
            subscriptions: IndexMap::new(),
            patterns: IndexMap::new(),
            endpoints: IndexMap::new(),
            clients: IndexMap::new(),
            routing_map: HashMap::new(),
            has_backing: false,
        }
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
    pub fn has_subscribers(&self, pattern: &str) -> bool {
        self.matching_handlers(&Ustr::from(pattern))
            .next()
            .is_some()
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

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn is_registered(&self, endpoint: &str) -> bool {
        self.endpoints.contains_key(&Ustr::from(endpoint))
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn is_subscribed(&self, topic: &str, handler: ShareableMessageHandler) -> bool {
        let sub = Subscription::new(Ustr::from(topic), handler, None);
        self.subscriptions.contains_key(&sub)
    }

    /// Close the message bus which will close the sender channel and join the thread.
    pub const fn close(&self) -> anyhow::Result<()> {
        // TODO: Integrate the backing database
        Ok(())
    }

    /// Registers a new [`DataClientAdaptor`]
    pub fn register_client(&mut self, client: DataClientAdapter, routing: Option<Venue>) {
        if let Some(routing) = routing {
            self.routing_map.insert(routing, client.client_id());
            log::info!("Set client {} routing for {routing}", client.client_id());
        }

        log::info!("Registered client {}", client.client_id());
        self.clients.insert(client.client_id, client);
    }

    /// Deregisters a [`DataClientAdaptor`]
    pub fn deregister_client(&mut self, client_id: &ClientId) {
        // TODO: We could return a `Result` but then this is part of system wiring and instead of
        // propagating results all over the place it may be cleaner to just immediately fail
        // for these sorts of design-time errors?
        // correctness::check_key_in_map(&client_id, &self.clients, "client_id", "clients").unwrap();

        self.clients.shift_remove(client_id);
        log::info!("Deregistered client {client_id}");
    }

    fn get_client(&self, client_id: &ClientId, venue: Venue) -> Option<&DataClientAdapter> {
        match self.clients.get(client_id) {
            Some(client) => Some(client),
            None => self
                .routing_map
                .get(&venue)
                .and_then(|client_id: &ClientId| self.clients.get(client_id)),
        }
    }

    /// Registers the given `handler` for the `endpoint` address.
    pub fn register(&mut self, endpoint: &str, handler: ShareableMessageHandler) {
        // Updates value if key already exists
        self.endpoints.insert(Ustr::from(endpoint), handler);
    }

    /// Deregisters the given `handler` for the `endpoint` address.
    pub fn deregister(&mut self, endpoint: &str) {
        // Removes entry if it exists for endpoint
        self.endpoints.shift_remove(&Ustr::from(endpoint));
    }

    /// Subscribes the given `handler` to the `topic`.
    pub fn subscribe(
        &mut self,
        topic: &str,
        handler: ShareableMessageHandler,
        priority: Option<u8>,
    ) {
        let topic = Ustr::from(topic);
        let sub = Subscription::new(topic, handler, priority);

        if self.subscriptions.contains_key(&sub) {
            error!("{sub:?} already exists.");
            return;
        }

        // Find existing patterns which match this topic
        let mut matches = Vec::new();
        for (pattern, subs) in &mut self.patterns {
            if is_matching(&topic, pattern) {
                subs.push(sub.clone());
                subs.sort();
                // subs.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.cmp(b)));
                matches.push(*pattern);
            }
        }

        matches.sort();

        self.subscriptions.insert(sub, matches);
    }

    /// Unsubscribes the given `handler` from the `topic`.
    pub fn unsubscribe(&mut self, topic: &str, handler: ShareableMessageHandler) {
        let sub = Subscription::new(Ustr::from(topic), handler, None);
        self.subscriptions.shift_remove(&sub);
    }

    /// Returns the handler for the given `endpoint`.
    #[must_use]
    pub fn get_endpoint(&self, endpoint: &Ustr) -> Option<&ShareableMessageHandler> {
        self.endpoints.get(&Ustr::from(endpoint))
    }

    #[must_use]
    pub fn matching_subscriptions<'a>(&'a self, pattern: &'a Ustr) -> Vec<&'a Subscription> {
        let mut matching_subs: Vec<&'a Subscription> = Vec::new();

        // Collect matching subscriptions from direct subscriptions
        matching_subs.extend(self.subscriptions.iter().filter_map(|(sub, _)| {
            if is_matching(&sub.topic, pattern) {
                Some(sub)
            } else {
                None
            }
        }));

        // Collect matching subscriptions from pattern-based subscriptions
        // TODO: Improve efficiency of this
        for subs in self.patterns.values() {
            let filtered_subs: Vec<&Subscription> = subs
                .iter()
                // .filter(|sub| is_matching(&sub.topic, pattern))
                // .filter(|sub| !matching_subs.contains(sub) && is_matching(&sub.topic, pattern))
                .collect();

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

    /// Sends a message to an endpoint.
    pub fn send(&self, endpoint: &str, message: &dyn Any) {
        if let Some(handler) = self.get_endpoint(&Ustr::from(endpoint)) {
            handler.0.handle(message);
        }
    }

    /// Publish a message to a topic.
    pub fn publish(&self, topic: &str, message: &dyn Any) {
        let topic = Ustr::from(topic);
        let matching_subs = self.matching_subscriptions(&topic);

        for sub in matching_subs {
            sub.handler.0.handle(message);
        }
    }
}

/// Data specific functions
impl MessageBus {
    /// Send a [`DataRequest`] to an endpoint that must be a [`DataClient`] implementation.
    pub fn send_data_request(&self, message: DataRequest) {
        // TODO: log error
        if let Some(client) = self.get_client(&message.client_id, message.venue) {
            let _ = client.request(message);
        }
    }

    /// Send a [`SubscriptionCommand`] to an endpoint that must be a [`DataClient`] implementation.
    pub fn send_subscription_command(&self, message: SubscriptionCommand) {
        if let Some(client) = self.get_client(&message.client_id, message.venue) {
            client.through_execute(message);
        }
    }

    /// Send a [`DataResponse`] to an endpoint that must be an actor.
    pub fn send_response(&self, message: DataResponse) {
        if let Some(handler) = self.get_endpoint(&message.client_id.inner()) {
            handler.0.handle_response(message);
        }
    }

    /// Publish [`Data`] to a topic.
    pub fn publish_data(&self, topic: &str, message: Data) {
        let topic = Ustr::from(topic);
        let matching_subs = self.matching_subscriptions(&topic);

        for sub in matching_subs {
            sub.handler.0.handle_data(&message);
        }
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {

    use nautilus_core::uuid::UUID4;
    use rstest::*;

    use super::*;
    use crate::msgbus::stubs::{
        get_call_check_shareable_handler, get_stub_shareable_handler, CallCheckMessageHandler,
    };

    fn stub_msgbus() -> MessageBus {
        MessageBus::new(TraderId::from("trader-001"), UUID4::new(), None, None)
    }

    #[rstest]
    fn test_new() {
        let trader_id = TraderId::from("trader-001");
        let msgbus = MessageBus::new(trader_id, UUID4::new(), None, None);

        assert_eq!(msgbus.trader_id, trader_id);
        assert_eq!(msgbus.name, stringify!(MessageBus));
    }

    #[rstest]
    fn test_endpoints_when_no_endpoints() {
        let msgbus = stub_msgbus();

        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_topics_when_no_subscriptions() {
        let msgbus = stub_msgbus();

        assert!(msgbus.topics().is_empty());
        assert!(!msgbus.has_subscribers("my-topic"));
    }

    #[rstest]
    fn test_is_subscribed_when_no_subscriptions() {
        let msgbus = stub_msgbus();

        let handler_id = Ustr::from("1");
        let handler = get_stub_shareable_handler(handler_id);

        assert!(!msgbus.is_subscribed("my-topic", handler));
    }

    #[rstest]
    fn test_is_registered_when_no_registrations() {
        let msgbus = stub_msgbus();

        assert!(!msgbus.is_registered("MyEndpoint"));
    }

    #[rstest]
    fn test_regsiter_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let handler_id = Ustr::from("1");
        let handler = get_stub_shareable_handler(handler_id);

        msgbus.register(endpoint, handler);

        assert_eq!(msgbus.endpoints(), vec!["MyEndpoint".to_string()]);
        assert!(msgbus.get_endpoint(&Ustr::from(endpoint)).is_some());
    }

    #[rstest]
    fn test_endpoint_send() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let handler_id = Ustr::from("1");
        let handler = get_call_check_shareable_handler(handler_id);

        msgbus.register(endpoint, handler.clone());
        assert!(msgbus.get_endpoint(&Ustr::from(endpoint)).is_some());

        // check if the handler called variable is false
        assert!(!handler
            .0
            .as_ref()
            .as_any()
            .downcast_ref::<CallCheckMessageHandler>()
            .unwrap()
            .was_called());

        // Send a message to the endpoint
        msgbus.send(endpoint, &"Test Message");

        // Check if the handler was called
        assert!(handler
            .0
            .as_ref()
            .as_any()
            .downcast_ref::<CallCheckMessageHandler>()
            .unwrap()
            .was_called());
    }

    #[rstest]
    fn test_deregsiter_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let handler_id = Ustr::from("1");
        let handler = get_stub_shareable_handler(handler_id);

        msgbus.register(endpoint, handler);
        msgbus.deregister(endpoint);

        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let handler_id = Ustr::from("1");
        let handler = get_stub_shareable_handler(handler_id);

        msgbus.subscribe(topic, handler, Some(1));

        assert!(msgbus.has_subscribers(topic));
        assert_eq!(msgbus.topics(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let handler_id = Ustr::from("1");
        let handler = get_stub_shareable_handler(handler_id);

        msgbus.subscribe(topic, handler.clone(), None);
        msgbus.unsubscribe(topic, handler);

        assert!(!msgbus.has_subscribers(topic));
        assert!(msgbus.topics().is_empty());
    }

    #[rstest]
    fn test_matching_subscriptions() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let handler_id1 = Ustr::from("1");
        let handler1 = get_stub_shareable_handler(handler_id1);

        let handler_id2 = Ustr::from("2");
        let handler2 = get_stub_shareable_handler(handler_id2);

        let handler_id3 = Ustr::from("3");
        let handler3 = get_stub_shareable_handler(handler_id3);

        let handler_id4 = Ustr::from("4");
        let handler4 = get_stub_shareable_handler(handler_id4);

        msgbus.subscribe(topic, handler1, None);
        msgbus.subscribe(topic, handler2, None);
        msgbus.subscribe(topic, handler3, Some(1));
        msgbus.subscribe(topic, handler4, Some(2));
        let topic_ustr = Ustr::from(topic);
        let subs = msgbus.matching_subscriptions(&topic_ustr);

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
    fn test_is_matching(#[case] topic: &str, #[case] pattern: &str, #[case] expected: bool) {
        assert_eq!(
            is_matching(&Ustr::from(topic), &Ustr::from(pattern)),
            expected
        );
    }
}

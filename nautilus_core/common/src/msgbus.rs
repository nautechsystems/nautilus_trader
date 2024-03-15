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

use std::{
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    sync::mpsc::{channel, Receiver, SendError, Sender},
    thread,
};

use indexmap::IndexMap;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use crate::handlers::MessageHandler;
#[cfg(feature = "redis")]
use crate::redis::handle_messages_with_redis;

// Represents a subscription to a particular topic.
//
// This is an internal class intended to be used by the message bus to organize
// topics and their subscribers.
#[derive(Clone, Debug)]
pub struct Subscription {
    pub handler: MessageHandler,
    pub topic: Ustr,
    pub sequence: usize,
    pub priority: u8,
}

impl Subscription {
    #[must_use]
    pub fn new(
        topic: Ustr,
        handler: MessageHandler,
        sequence: usize,
        priority: Option<u8>,
    ) -> Self {
        Self {
            topic,
            handler,
            sequence,
            priority: priority.unwrap_or(0),
        }
    }
}

impl PartialEq<Self> for Subscription {
    fn eq(&self, other: &Self) -> bool {
        self.topic == other.topic && self.handler.handler_id == other.handler.handler_id
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
        match other.priority.cmp(&self.priority) {
            std::cmp::Ordering::Equal => self.sequence.cmp(&other.sequence),
            other => other,
        }
    }
}

impl Hash for Subscription {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.topic.hash(state);
        self.handler.handler_id.hash(state);
    }
}

/// Represents a bus message including a topic and payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusMessage {
    /// The topic to publish on.
    pub topic: String,
    /// The serialized payload for the message.
    pub payload: Vec<u8>,
}

impl fmt::Display for BusMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}",
            self.topic,
            String::from_utf8_lossy(&self.payload)
        )
    }
}

/// Provides a generic message bus to facilitate various messaging patterns.
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
#[derive(Clone)]
pub struct MessageBus {
    /// The trader ID associated with the message bus.
    pub trader_id: TraderId,
    /// The instance ID associated with the message bus.
    pub instance_id: UUID4,
    /// The name for the message bus.
    pub name: String,
    // The count of messages sent through the bus.
    pub sent_count: u64,
    // The count of requests processed by the bus.
    pub req_count: u64,
    // The count of responses processed by the bus.
    pub res_count: u64,
    /// The count of messages published by the bus.
    pub pub_count: u64,
    /// If the message bus is backed by a database.
    pub has_backing: bool,
    tx: Option<Sender<BusMessage>>,
    /// mapping from topic to the corresponding handler
    /// a topic can be a string with wildcards
    /// * '?' - any character
    /// * '*' - any number of any characters
    subscriptions: IndexMap<Subscription, Vec<Ustr>>,
    /// maps a pattern to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    patterns: IndexMap<Ustr, Vec<Subscription>>,
    /// handles a message or a request destined for a specific endpoint.
    endpoints: IndexMap<Ustr, MessageHandler>,
    /// Relates a request with a response
    /// a request maps it's id to a handler so that a response
    /// with the same id can later be handled.
    correlation_index: IndexMap<UUID4, MessageHandler>,
}

impl MessageBus {
    /// Initializes a new instance of the [`MessageBus`].
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        name: Option<String>,
        config: Option<HashMap<String, serde_json::Value>>,
    ) -> anyhow::Result<Self> {
        let config = config.unwrap_or_default();
        let has_backing = config
            .get("database")
            .map_or(false, |v| v != &serde_json::Value::Null);
        let tx = if has_backing {
            let (tx, rx) = channel::<BusMessage>();
            let _join_handler = thread::Builder::new()
                .name("msgbus".to_string())
                .spawn(move || Self::handle_messages(rx, trader_id, instance_id, config))
                .expect("Error spawning `msgbus` thread");
            Some(tx)
        } else {
            None
        };

        Ok(Self {
            tx,
            trader_id,
            instance_id,
            name: name.unwrap_or_else(|| stringify!(MessageBus).to_owned()),
            sent_count: 0,
            req_count: 0,
            res_count: 0,
            pub_count: 0,
            subscriptions: IndexMap::new(),
            patterns: IndexMap::new(),
            endpoints: IndexMap::new(),
            correlation_index: IndexMap::new(),
            has_backing,
        })
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

    /// Returns the active correlation IDs.
    #[must_use]
    pub fn correlation_ids(&self) -> Vec<&UUID4> {
        self.correlation_index.keys().collect()
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
            .map(|s| s.handler.handler_id.as_str())
            .collect()
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn is_registered(&self, endpoint: &str) -> bool {
        self.endpoints.contains_key(&Ustr::from(endpoint))
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn is_subscribed(&self, topic: &str, handler: MessageHandler) -> bool {
        let sub = Subscription::new(Ustr::from(topic), handler, self.subscriptions.len(), None);
        self.subscriptions.contains_key(&sub)
    }

    /// Returns whether there is a pending request for the given `request_id`.
    #[must_use]
    pub fn is_pending_response(&self, request_id: &UUID4) -> bool {
        self.correlation_index.contains_key(request_id)
    }

    /// Registers the given `handler` for the `endpoint` address.
    pub fn register(&mut self, endpoint: &str, handler: MessageHandler) {
        // Updates value if key already exists
        self.endpoints.insert(Ustr::from(endpoint), handler);
    }

    /// Deregisters the given `handler` for the `endpoint` address.
    pub fn deregister(&mut self, endpoint: &str) {
        // Removes entry if it exists for endpoint
        self.endpoints.shift_remove(&Ustr::from(endpoint));
    }

    /// Subscribes the given `handler` to the `topic`.
    pub fn subscribe(&mut self, topic: &str, handler: MessageHandler, priority: Option<u8>) {
        let topic = Ustr::from(topic);
        let sub = Subscription::new(topic, handler, self.subscriptions.len(), priority);

        if self.subscriptions.contains_key(&sub) {
            // TODO: Implement proper logging
            println!("{sub:?} already exists.");
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
    pub fn unsubscribe(&mut self, topic: &str, handler: MessageHandler) {
        let sub = Subscription::new(Ustr::from(topic), handler, self.subscriptions.len(), None);
        self.subscriptions.shift_remove(&sub);
    }

    /// Returns the handler for the given `endpoint`.
    #[must_use]
    pub fn get_endpoint(&self, endpoint: &Ustr) -> Option<&MessageHandler> {
        self.endpoints.get(&Ustr::from(endpoint))
    }

    /// Returns the handler for the request `endpoint` and adds the request ID to the internal
    /// correlation index to match with the expected response.
    #[must_use]
    pub fn request_handler(
        &mut self,
        endpoint: &Ustr,
        request_id: UUID4,
        response_handler: MessageHandler,
    ) -> Option<&MessageHandler> {
        if let Some(handler) = self.endpoints.get(endpoint) {
            self.correlation_index.insert(request_id, response_handler);
            Some(handler)
        } else {
            None
        }
    }

    /// Returns the handler for the matching correlation ID (if found).
    #[must_use]
    pub fn correlation_id_handler(&mut self, correlation_id: &UUID4) -> Option<&MessageHandler> {
        self.correlation_index.get(correlation_id)
    }

    /// Returns the handler for the matching response `endpoint` based on the internal correlation
    /// index.
    #[must_use]
    pub fn response_handler(&mut self, correlation_id: &UUID4) -> Option<MessageHandler> {
        self.correlation_index.shift_remove(correlation_id)
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
    ) -> impl Iterator<Item = &'a MessageHandler> {
        self.subscriptions.iter().filter_map(move |(sub, _)| {
            if is_matching(&sub.topic, pattern) {
                Some(&sub.handler)
            } else {
                None
            }
        })
    }

    pub fn publish_external(&self, topic: String, payload: Vec<u8>) {
        if let Some(tx) = &self.tx {
            let msg = BusMessage { topic, payload };
            if let Err(SendError(e)) = tx.send(msg) {
                eprintln!("Error publishing external message: {e}");
            }
        } else {
            eprintln!("Error publishing external message: no tx channel");
        }
    }

    fn handle_messages(
        rx: Receiver<BusMessage>,
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<()> {
        let database_config = config
            .get("database")
            .expect("No `MessageBusConfig` `database` config specified");
        let backing_type = database_config
            .get("type")
            .expect("No `MessageBusConfig` database config `type` specified")
            .as_str()
            .expect("`MessageBusConfig` database `type` must be a valid string");

        match backing_type {
            "redis" => handle_messages_with_redis_if_enabled(rx, trader_id, instance_id, config),
            other => panic!("Unsupported message bus backing database type '{other}'"),
        }
    }
}

/// Handles messages using Redis if the `redis` feature is enabled.
#[cfg(feature = "redis")]
fn handle_messages_with_redis_if_enabled(
    rx: Receiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: HashMap<String, Value>,
) -> anyhow::Result<()> {
    handle_messages_with_redis(rx, trader_id, instance_id, config)
}

/// Handles messages using a default method if the "redis" feature is not enabled.
#[cfg(not(feature = "redis"))]
fn handle_messages_with_redis_if_enabled(
    _rx: Receiver<BusMessage>,
    _trader_id: TraderId,
    _instance_id: UUID4,
    _config: HashMap<String, Value>,
) {
    panic!("`redis` feature is not enabled");
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
#[cfg(not(feature = "python"))]
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_core::{message::Message, uuid::UUID4};
    use rstest::*;

    use super::*;
    use crate::handlers::{MessageHandler, SafeMessageCallback};

    fn stub_msgbus() -> MessageBus {
        MessageBus::new(TraderId::from("trader-001"), UUID4::new(), None, None)
    }

    fn stub_rust_callback() -> SafeMessageCallback {
        SafeMessageCallback {
            callback: Arc::new(|m: Message| {
                format!("{m:?}");
            }),
        }
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

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, Some(callback));

        assert!(!msgbus.is_subscribed("my-topic", handler));
    }

    #[rstest]
    fn test_is_registered_when_no_registrations() {
        let msgbus = stub_msgbus();

        assert!(!msgbus.is_registered("MyEndpoint"));
    }

    #[rstest]
    fn test_is_pending_response_when_no_requests() {
        let msgbus = stub_msgbus();

        assert!(!msgbus.is_pending_response(&UUID4::default()));
    }

    #[rstest]
    fn test_regsiter_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, Some(callback));

        msgbus.register(endpoint, handler);

        assert_eq!(msgbus.endpoints(), vec!["MyEndpoint".to_string()]);
        assert!(msgbus.get_endpoint(&Ustr::from(endpoint)).is_some());
    }

    #[rstest]
    fn test_deregsiter_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, Some(callback));

        msgbus.register(endpoint, handler);
        msgbus.deregister(endpoint);

        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, Some(callback));

        msgbus.subscribe(topic, handler, Some(1));

        assert!(msgbus.has_subscribers(topic));
        assert_eq!(msgbus.topics(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, Some(callback));

        msgbus.subscribe(topic, handler.clone(), None);
        msgbus.unsubscribe(topic, handler);

        assert!(!msgbus.has_subscribers(topic));
        assert!(msgbus.topics().is_empty());
    }

    #[rstest]
    fn test_request_handler() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";
        let request_id = UUID4::new();

        let callback = stub_rust_callback();
        let handler_id1 = Ustr::from("1");
        let handler1 = MessageHandler::new(handler_id1, Some(callback));

        msgbus.register(endpoint, handler1.clone());

        let callback = stub_rust_callback();
        let handler_id2 = Ustr::from("1");
        let handler2 = MessageHandler::new(handler_id2, Some(callback));

        assert_eq!(
            msgbus.request_handler(&Ustr::from(endpoint), request_id, handler2),
            Some(&handler1)
        );
    }

    #[rstest]
    fn test_response_handler() {
        let mut msgbus = stub_msgbus();
        let correlation_id = UUID4::new();

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, Some(callback));

        msgbus
            .correlation_index
            .insert(correlation_id, handler.clone());

        assert_eq!(msgbus.response_handler(&correlation_id), Some(handler));
    }

    #[rstest]
    fn test_matching_subscriptions() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let callback = stub_rust_callback();
        let handler_id1 = Ustr::from("1");
        let handler1 = MessageHandler::new(handler_id1, Some(callback.clone()));

        let handler_id2 = Ustr::from("2");
        let handler2 = MessageHandler::new(handler_id2, Some(callback.clone()));

        let handler_id3 = Ustr::from("3");
        let handler3 = MessageHandler::new(handler_id3, Some(callback.clone()));

        let handler_id4 = Ustr::from("4");
        let handler4 = MessageHandler::new(handler_id4, Some(callback));

        msgbus.subscribe(topic, handler1, None);
        msgbus.subscribe(topic, handler2, None);
        msgbus.subscribe(topic, handler3, Some(1));
        msgbus.subscribe(topic, handler4, Some(2));
        let topic_ustr = Ustr::from(topic);
        let subs = msgbus.matching_subscriptions(&topic_ustr);

        assert_eq!(subs.len(), 4);
        assert_eq!(subs[0].handler.handler_id, handler_id4);
        assert_eq!(subs[1].handler.handler_id, handler_id3);
        assert_eq!(subs[2].handler.handler_id, handler_id1);
        assert_eq!(subs[3].handler.handler_id, handler_id2);
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

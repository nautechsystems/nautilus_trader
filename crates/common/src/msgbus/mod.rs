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
pub mod runner;
pub mod switchboard;

use core::ops::Coroutine;
use std::{
    any::Any,
    collections::HashMap,
    fmt::Debug,
    hash::{Hash, Hasher},
    pin::Pin,
    rc::Rc,
};

use indexmap::IndexMap;
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;
use switchboard::MessagingSwitchboard;
use ustr::Ustr;

pub const CLOSE_TOPIC: &str = "CLOSE";

/// Commands for the message bus runner.
pub enum Command {
    /// Send a message to an endpoint.
    Send {
        topic: Ustr,
        // Boxed dynamic message.
        msg: Rc<dyn Any>,
    },
    /// Publish a message to a topic.
    Publish { pattern: String, msg: Rc<dyn Any> },
    /// Register an endpoint subscription
    Register(Subscription),
    /// Deregister an endpoint subscription
    Deregister(Ustr),
    /// Subscribe to a topic
    Subscribe(Subscription),
    /// Unsubscribe from a topic
    Unsubscribe((Ustr, Ustr)),
}

/// A coroutine for the message bus can receive any message and yield message bus commands.
pub type HandlerCoroutine = Pin<Box<dyn Coroutine<Rc<dyn Any>, Yield = Command, Return = ()>>>;
/// A wrapper function that creates a new coroutine each time it is called.
pub type HandlerFn = Rc<dyn Fn() -> HandlerCoroutine>;

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
    /// A function to create handler coroutines to handle messages.
    pub handler_fn: HandlerFn,
    /// Store a copy of the handler ID for faster equality checks.
    pub handler_id: Ustr,
    /// The subscription topic for receiving direct and published messages.
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
        handler: HandlerFn,
        handler_id: T,
        priority: Option<u8>,
    ) -> Self {
        Self {
            handler_id: Ustr::from(handler_id.as_ref()),
            topic: Ustr::from(topic.as_ref()),
            handler_fn: handler,
            priority: priority.unwrap_or(0),
        }
    }

    /// Creates a new dummy [`Subscription`] instance.
    ///
    /// It is useful for equating keys in the mapping as it only needs
    /// the topic and handler ID to be equal.
    pub fn dummy<T: AsRef<str>>(topic: T, handler_id: T) -> Self {
        Self::new(
            topic,
            Rc::new(move || {
                Box::pin(
                    #[coroutine]
                    move |_msg: Rc<dyn Any>| {},
                )
            }),
            handler_id,
            None,
        )
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
    endpoints: IndexMap<Ustr, Subscription>,
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
    pub fn is_subscribed<T: AsRef<str>>(&self, topic: T, handler_id: T) -> bool {
        let sub = Subscription::dummy(topic, handler_id);
        self.subscriptions.contains_key(&sub)
    }

    /// Close the message bus which will close the sender channel and join the thread.
    pub const fn close(&self) -> anyhow::Result<()> {
        // TODO: Integrate the backing database
        Ok(())
    }

    /// Registers the given `handler` for the `endpoint` address.
    pub fn register(&mut self, subscription: Subscription) {
        log::debug!(
            "Registering endpoint '{}' with handler ID {} at {}",
            subscription.topic,
            subscription.handler_id,
            self.memory_address(),
        );
        // Updates value if key already exists
        self.endpoints.insert(subscription.topic, subscription);
    }

    /// Deregisters the given `handler` for the `endpoint` address.
    pub fn deregister(&mut self, endpoint: &Ustr) {
        log::debug!(
            "Deregistering endpoint '{endpoint}' at {}",
            self.memory_address()
        );
        // Removes entry if it exists for endpoint
        self.endpoints.shift_remove(endpoint);
    }

    /// Subscribes the given `handler` to the `topic`.
    pub fn subscribe(&mut self, subscription: Subscription) {
        log::debug!(
            "Subscribing for topic '{}' at {}",
            subscription.topic,
            self.memory_address(),
        );
        if self.subscriptions.contains_key(&subscription) {
            log::error!("{subscription:?} already exists.");
            return;
        }

        // Find existing patterns which match this topic
        let mut matches = Vec::new();
        for (pattern, subs) in &mut self.patterns {
            if is_matching(&subscription.topic, pattern) {
                subs.push(subscription.clone());
                subs.sort();
                // subs.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.cmp(b)));
                matches.push(*pattern);
            }
        }

        matches.sort();

        self.subscriptions.insert(subscription, matches);
    }

    /// Unsubscribes the given `handler` from the `topic`.
    pub fn unsubscribe<T: AsRef<str>>(&mut self, topic: T, handler_id: T) {
        log::debug!(
            "Unsubscribing for topic '{}' at {}",
            topic.as_ref(),
            self.memory_address(),
        );
        let sub = Subscription::dummy(topic, handler_id);
        self.subscriptions.shift_remove(&sub);
    }

    /// Returns the handler for the given `endpoint`.
    #[must_use]
    pub fn get_endpoint<T: AsRef<str>>(&self, endpoint: T) -> Option<&Subscription> {
        self.endpoints.get(&Ustr::from(endpoint.as_ref()))
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
    ) -> impl Iterator<Item = &'a Subscription> {
        self.subscriptions.iter().filter_map(move |(sub, _)| {
            if is_matching(&sub.topic, pattern) {
                Some(sub)
            } else {
                None
            }
        })
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
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use nautilus_core::UUID4;
    use rstest::*;

    use super::*;

    pub fn stub_msgbus() -> MessageBus {
        MessageBus::new(TraderId::from("trader-001"), UUID4::new(), None, None)
    }

    // Create a coroutine that sets a flag when called
    fn create_call_check_coroutine() -> (HandlerFn, Arc<AtomicBool>) {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let handler_fn = Rc::new(move || {
            let called = called_clone.clone();
            Box::pin(
                #[coroutine]
                move |_msg: Rc<dyn Any>| {
                    called.store(true, Ordering::SeqCst);
                },
            ) as HandlerCoroutine
        });

        (handler_fn, called)
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
        let handler_id = Ustr::from("test-handler");

        assert!(!msgbus.is_subscribed("my-topic", &handler_id));
    }

    #[rstest]
    fn test_is_registered_when_no_registrations() {
        let msgbus = stub_msgbus();
        assert!(!msgbus.is_registered("MyEndpoint"));
    }

    #[rstest]
    fn test_register_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";
        let handler_id = Ustr::from(endpoint);

        // Use Subscription::dummy to create a subscription
        let subscription = Subscription::dummy(endpoint, &handler_id);

        msgbus.register(subscription);
        assert_eq!(msgbus.endpoints(), vec![endpoint]);
        assert!(msgbus.get_endpoint(endpoint).is_some());
    }

    #[rstest]
    fn test_deregister_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = Ustr::from("MyEndpoint");

        // Use Subscription::dummy to create a subscription
        let subscription = Subscription::dummy(endpoint.as_str(), &endpoint);

        msgbus.register(subscription);
        msgbus.deregister(&endpoint);
        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";
        let handler_id = Ustr::from("test-handler");

        // Use Subscription::dummy to create a subscription with priority
        let mut subscription = Subscription::dummy(topic, &handler_id);
        subscription.priority = 1;

        msgbus.subscribe(subscription);
        assert!(msgbus.has_subscribers(topic));
        assert_eq!(msgbus.topics(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";
        let handler_id = Ustr::from("test-handler");

        // Use Subscription::dummy to create a subscription
        let subscription = Subscription::dummy(topic, &handler_id);

        msgbus.subscribe(subscription);
        msgbus.unsubscribe(topic, &handler_id);
        assert!(!msgbus.has_subscribers(topic));
        assert!(msgbus.topics().is_empty());
    }

    #[rstest]
    fn test_matching_subscriptions() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let handler_id1 = Ustr::from("1");
        let handler_id2 = Ustr::from("2");
        let handler_id3 = Ustr::from("3");
        let handler_id4 = Ustr::from("4");

        // Use Subscription::dummy to create subscriptions with different priorities
        let subscription1 = Subscription::dummy(topic, &handler_id1);
        let subscription2 = Subscription::dummy(topic, &handler_id2);
        let mut subscription3 = Subscription::dummy(topic, &handler_id3);
        subscription3.priority = 1;
        let mut subscription4 = Subscription::dummy(topic, &handler_id4);
        subscription4.priority = 2;

        msgbus.subscribe(subscription1);
        msgbus.subscribe(subscription2);
        msgbus.subscribe(subscription3);
        msgbus.subscribe(subscription4);

        let topic = Ustr::from(topic);
        let subs = msgbus.matching_subscriptions(&topic);

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

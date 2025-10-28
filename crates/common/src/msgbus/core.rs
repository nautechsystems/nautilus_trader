// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 2Nautech Systems Pty Ltd. All rights reserved.
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
    cell::RefCell,
    collections::HashMap,
    fmt::{self, Display},
    hash::{Hash, Hasher},
    ops::Deref,
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use handler::ShareableMessageHandler;
use indexmap::IndexMap;
use matching::is_matching_backtracking;
use nautilus_core::{
    UUID4,
    correctness::{FAILED, check_predicate_true, check_valid_string_ascii},
};
use nautilus_model::identifiers::TraderId;
use switchboard::MessagingSwitchboard;
use ustr::Ustr;

use super::{handler, matching, set_message_bus, switchboard};

#[inline(always)]
fn check_fully_qualified_string(value: &Ustr, key: &str) -> anyhow::Result<()> {
    check_predicate_true(
        !value.chars().any(|c| c == '*' || c == '?'),
        &format!("{key} `value` contained invalid characters, was {value}"),
    )
}

/// Pattern is a string pattern for a subscription with special characters for pattern matching.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pattern;

/// Topic is a fully qualified string for publishing data.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Topic;

/// Endpoint is a fully qualified string for sending data.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Endpoint;

/// A message bus string type. It can be a pattern or a topic.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MStr<T> {
    value: Ustr,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Display for MStr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl<T> Deref for MStr<T> {
    type Target = Ustr;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl MStr<Pattern> {
    /// Create a new pattern from a string.
    pub fn pattern<T: AsRef<str>>(value: T) -> Self {
        let value = Ustr::from(value.as_ref());

        Self {
            value,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: AsRef<str>> From<T> for MStr<Pattern> {
    fn from(value: T) -> Self {
        Self::pattern(value)
    }
}

impl From<MStr<Topic>> for MStr<Pattern> {
    fn from(value: MStr<Topic>) -> Self {
        Self {
            value: value.value,
            _marker: std::marker::PhantomData,
        }
    }
}

impl MStr<Topic> {
    /// Create a new topic from a fully qualified string.
    ///
    /// # Errors
    ///
    /// Returns an error if the topic has white space or invalid characters.
    pub fn topic<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let topic = Ustr::from(value.as_ref());
        check_valid_string_ascii(value, stringify!(value))?;
        check_fully_qualified_string(&topic, stringify!(Topic))?;

        Ok(Self {
            value: topic,
            _marker: std::marker::PhantomData,
        })
    }
}

impl<T: AsRef<str>> From<T> for MStr<Topic> {
    fn from(value: T) -> Self {
        Self::topic(value).expect(FAILED)
    }
}

impl MStr<Endpoint> {
    /// Create a new endpoint from a fully qualified string.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint has white space or invalid characters.
    pub fn endpoint<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let endpoint = Ustr::from(value.as_ref());
        check_valid_string_ascii(value, stringify!(value))?;
        check_fully_qualified_string(&endpoint, stringify!(Endpoint))?;

        Ok(Self {
            value: endpoint,
            _marker: std::marker::PhantomData,
        })
    }
}

impl<T: AsRef<str>> From<T> for MStr<Endpoint> {
    fn from(value: T) -> Self {
        Self::endpoint(value).expect(FAILED)
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
    pub pattern: MStr<Pattern>,
    /// The priority for the subscription determines the ordering of handlers receiving
    /// messages being processed, higher priority handlers will receive messages before
    /// lower priority handlers.
    pub priority: u8,
}

impl Subscription {
    /// Creates a new [`Subscription`] instance.
    #[must_use]
    pub fn new(
        pattern: MStr<Pattern>,
        handler: ShareableMessageHandler,
        priority: Option<u8>,
    ) -> Self {
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
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.pattern.cmp(&other.pattern))
            .then_with(|| self.handler_id.cmp(&other.handler_id))
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
    pub subscriptions: AHashSet<Subscription>,
    /// Maps a topic to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    pub topics: IndexMap<MStr<Topic>, Vec<Subscription>>,
    /// Index of endpoint addresses and their handlers.
    pub endpoints: IndexMap<MStr<Endpoint>, ShareableMessageHandler>,
    /// Index of request correlation IDs and their response handlers.
    pub correlation_index: AHashMap<UUID4, ShareableMessageHandler>,
}

// MessageBus is designed for single-threaded use within each async runtime.
// Thread-local storage ensures each thread gets its own instance, eliminating
// the need for unsafe Send/Sync implementations that were previously required
// for global static storage.

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
            subscriptions: AHashSet::new(),
            topics: IndexMap::new(),
            endpoints: IndexMap::new(),
            correlation_index: AHashMap::new(),
            has_backing: false,
        }
    }

    /// Returns the memory address of this instance as a hexadecimal string.
    #[must_use]
    pub fn mem_address(&self) -> String {
        format!("{self:p}")
    }

    /// Returns the registered endpoint addresses.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&str> {
        self.endpoints.iter().map(|e| e.0.as_str()).collect()
    }

    /// Returns actively subscribed patterns.
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.pattern.as_str())
            .collect()
    }

    /// Returns whether there are subscribers for the `topic`.
    pub fn has_subscribers<T: AsRef<str>>(&self, topic: T) -> bool {
        self.subscriptions_count(topic) > 0
    }

    /// Returns the count of subscribers for the `topic`.
    ///
    /// # Panics
    ///
    /// Returns an error if the topic is not valid.
    #[must_use]
    pub fn subscriptions_count<T: AsRef<str>>(&self, topic: T) -> usize {
        let topic = MStr::<Topic>::topic(topic).expect(FAILED);
        self.topics
            .get(&topic)
            .map_or_else(|| self.find_topic_matches(topic).len(), |subs| subs.len())
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

    /// Returns whether the endpoint is registered.
    ///
    /// # Panics
    ///
    /// Returns an error if the endpoint is not valid topic string.
    #[must_use]
    pub fn is_registered<T: AsRef<str>>(&self, endpoint: T) -> bool {
        let endpoint: MStr<Endpoint> = endpoint.into();
        self.endpoints.contains_key(&endpoint)
    }

    /// Returns whether the `handler` is subscribed to the `pattern`.
    #[must_use]
    pub fn is_subscribed<T: AsRef<str>>(
        &self,
        pattern: T,
        handler: ShareableMessageHandler,
    ) -> bool {
        let pattern = MStr::<Pattern>::pattern(pattern);
        let sub = Subscription::new(pattern, handler, None);
        self.subscriptions.contains(&sub)
    }

    /// Close the message bus which will close the sender channel and join the thread.
    ///
    /// # Errors
    ///
    /// This function never returns an error (TBD once backing database added).
    pub const fn close(&self) -> anyhow::Result<()> {
        // TODO: Integrate the backing database
        Ok(())
    }

    /// Returns the handler for the `endpoint`.
    #[must_use]
    pub fn get_endpoint(&self, endpoint: MStr<Endpoint>) -> Option<&ShareableMessageHandler> {
        self.endpoints.get(&endpoint)
    }

    /// Returns the handler for the `correlation_id`.
    #[must_use]
    pub fn get_response_handler(&self, correlation_id: &UUID4) -> Option<&ShareableMessageHandler> {
        self.correlation_index.get(correlation_id)
    }

    /// Finds the subscriptions with pattern matching the `topic`.
    pub(crate) fn find_topic_matches(&self, topic: MStr<Topic>) -> Vec<Subscription> {
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

    /// Finds the subscriptions which match the `topic` and caches the
    /// results in the `patterns` map.
    #[must_use]
    pub fn matching_subscriptions<T: AsRef<str>>(&mut self, topic: T) -> Vec<Subscription> {
        let topic = MStr::<Topic>::from(topic);
        self.inner_matching_subscriptions(topic)
    }

    pub(crate) fn inner_matching_subscriptions(&mut self, topic: MStr<Topic>) -> Vec<Subscription> {
        self.topics.get(&topic).cloned().unwrap_or_else(|| {
            let mut matches = self.find_topic_matches(topic);
            matches.sort();
            self.topics.insert(topic, matches.clone());
            matches
        })
    }

    /// Registers a response handler for a specific correlation ID.
    ///
    /// # Errors
    ///
    /// Returns an error if `handler` is already registered for the `correlation_id`.
    pub fn register_response_handler(
        &mut self,
        correlation_id: &UUID4,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<()> {
        if self.correlation_index.contains_key(correlation_id) {
            anyhow::bail!("Correlation ID <{correlation_id}> already has a registered handler");
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

    /// Registers message bus for the current thread.
    pub fn register_message_bus(self) -> Rc<RefCell<Self>> {
        let msgbus = Rc::new(RefCell::new(self));
        set_message_bus(msgbus.clone());
        msgbus
    }
}

impl Default for MessageBus {
    /// Creates a new default [`MessageBus`] instance.
    fn default() -> Self {
        Self::new(TraderId::from("TRADER-001"), UUID4::new(), None, None)
    }
}

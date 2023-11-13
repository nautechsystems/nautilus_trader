// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    hash::{Hash, Hasher},
};

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use ustr::Ustr;

use crate::handlers::MessageHandler;

// Represents a subscription to a particular topic.
//
// This is an internal class intended to be used by the message bus to organize
// topics and their subscribers.
#[derive(Clone)]
pub struct Subscription {
    pub handler: MessageHandler,
    topic: Ustr,
    priority: u8,
}

impl Subscription {
    pub fn new(topic: Ustr, handler: MessageHandler, priority: Option<u8>) -> Self {
        Self {
            topic,
            handler,
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
        self.priority.cmp(&other.priority)
    }
}

impl Hash for Subscription {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.topic.hash(state);
        self.handler.handler_id.hash(state);
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
#[allow(dead_code)]
pub struct MessageBus {
    /// The trader ID for the message bus.
    pub trader_id: TraderId,
    /// The name for the message bus.
    pub name: String,
    /// mapping from topic to the corresponding handler
    /// a topic can be a string with wildcards
    /// * '?' - any character
    /// * '*' - any number of any characters
    subscriptions: HashMap<Subscription, Vec<Ustr>>,
    /// maps a pattern to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    patterns: HashMap<Ustr, Vec<Subscription>>,
    /// handles a message or a request destined for a specific endpoint.
    endpoints: HashMap<Ustr, MessageHandler>,
    /// Relates a request with a response
    /// a request maps it's id to a handler so that a response
    /// with the same id can later be handled.
    correlation_index: HashMap<UUID4, MessageHandler>,
}

#[allow(dead_code)]
impl MessageBus {
    /// Initializes a new instance of the [`MessageBus`].
    #[must_use]
    pub fn new(trader_id: TraderId, name: Option<String>) -> Self {
        Self {
            trader_id,
            name: name.unwrap_or_else(|| stringify!(MessageBus).to_owned()),
            subscriptions: HashMap::new(),
            patterns: HashMap::new(),
            endpoints: HashMap::new(),
            correlation_index: HashMap::new(),
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

    /// Registers the given `handler` for the `endpoint` address.
    pub fn register(&mut self, endpoint: &str, handler: MessageHandler) {
        // Updates value if key already exists
        self.endpoints.insert(Ustr::from(endpoint), handler);
    }

    /// Deregisters the given `handler` for the `endpoint` address.
    pub fn deregister(&mut self, endpoint: &str) {
        // removes entry if it exists for endpoint
        self.endpoints.remove(&Ustr::from(endpoint));
    }

    /// Subscribes the given `handler` to the `topic`.
    pub fn subscribe(&mut self, topic: &str, handler: MessageHandler, priority: Option<u8>) {
        let sub = Subscription::new(Ustr::from(topic), handler, priority);

        if self.subscriptions.contains_key(&sub) {
            // TODO: log
            return;
        }

        // Find existing patterns which match this topic
        let mut matches = Vec::new();
        for (pattern, subs) in &mut self.patterns {
            if is_matching(&Ustr::from(topic), pattern) {
                subs.push(sub.clone());
                subs.sort(); // Sort in priority order
                matches.push(*pattern);
            }
        }

        self.subscriptions.insert(sub, matches);
    }

    /// Unsubscribes the given `handler` from the `topic`.
    pub fn unsubscribe(&mut self, topic: &str, handler: MessageHandler) {
        let sub = Subscription::new(Ustr::from(topic), handler, None);

        self.subscriptions.remove(&sub);
    }

    /// Returns the handler for the given `endpoint`.
    #[must_use]
    pub fn get_endpoint(&self, endpoint: &Ustr) -> Option<&MessageHandler> {
        self.endpoints.get(&Ustr::from(endpoint))
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    pub fn has_subscribers(&self, pattern: &str) -> bool {
        self.matching_handlers(&Ustr::from(pattern))
            .next()
            .is_some()
    }

    // #[allow(unused_variables)]
    // fn request(&mut self, endpoint: &String, request: &Message, callback: T) {
    //     match request {
    //         Message::Request { id, ts_init } => {
    //             if self.correlation_index.contains_key(id) {
    //                 todo!()
    //             } else {
    //                 self.correlation_index.insert(*id, callback);
    //                 if let Some(handler) = self.endpoints.get(endpoint) {
    //                     handler(request);
    //                 } else {
    //                     // TODO: log error
    //                 }
    //             }
    //         }
    //         _ => unreachable!(
    //             "message bus request should only be called with Message::Request variant"
    //         ),
    //     }
    // }
    //
    // #[allow(unused_variables)]
    // fn response(&mut self, response: &Message) {
    //     match response {
    //         Message::Response {
    //             id,
    //             ts_init,
    //             correlation_id,
    //         } => {
    //             if let Some(callback) = self.correlation_index.get(correlation_id) {
    //                 callback(response);
    //             } else {
    //                 // TODO: log error
    //             }
    //         }
    //         _ => unreachable!(
    //             "message bus response should only be called with Message::Response variant"
    //         ),
    //     }
    // }

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

    pub fn get_matching_subscriptions<'a>(
        &'a mut self,
        pattern: &'a Ustr,
    ) -> Vec<&'a Subscription> {
        let mut matching_subs = self
            .subscriptions
            .iter()
            .filter_map(|(sub, _)| {
                if is_matching(&sub.topic, pattern) {
                    Some(sub)
                } else {
                    None
                }
            })
            .collect::<Vec<&'a Subscription>>();

        for (p, subs) in &self.patterns {
            if is_matching(p, pattern) {
                matching_subs.extend(subs.iter());
            }
        }

        matching_subs.sort();
        matching_subs
    }
}

/// Match a topic and a string pattern
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
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
    use std::rc::Rc;

    use nautilus_core::message::Message;
    use rstest::*;

    use super::*;
    use crate::handlers::MessageHandler;

    fn stub_msgbus() -> MessageBus {
        MessageBus::new(TraderId::from("trader-001"), None)
    }

    fn stub_rust_callback() -> Rc<dyn Fn(Message)> {
        Rc::new(|m: Message| {
            format!("{m:?}");
        })
    }

    #[rstest]
    fn test_new() {
        let trader_id = TraderId::from("trader-001");
        let msgbus = MessageBus::new(trader_id, None);

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
    fn test_regsiter_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, None, Some(callback));

        msgbus.register(&endpoint, handler);

        assert_eq!(msgbus.endpoints(), vec!["MyEndpoint".to_string()]);
        assert!(msgbus.get_endpoint(&Ustr::from(&endpoint)).is_some());
    }

    #[rstest]
    fn test_deregsiter_endpoint() {
        let mut msgbus = stub_msgbus();
        let endpoint = "MyEndpoint";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, None, Some(callback));

        msgbus.register(&endpoint, handler);
        msgbus.deregister(&endpoint);

        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, None, Some(callback));

        msgbus.subscribe(&topic, handler, Some(1));

        assert!(msgbus.has_subscribers(&topic));
        assert_eq!(msgbus.topics(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let mut msgbus = stub_msgbus();
        let topic = "my-topic";

        let callback = stub_rust_callback();
        let handler_id = Ustr::from("1");
        let handler = MessageHandler::new(handler_id, None, Some(callback));

        msgbus.subscribe(&topic, handler.clone(), None);
        msgbus.unsubscribe(&topic, handler);

        assert!(!msgbus.has_subscribers(&topic));
        assert!(msgbus.topics().is_empty());
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

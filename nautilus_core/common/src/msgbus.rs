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
    rc::Rc,
};

use nautilus_core::{message::Message, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use ustr::Ustr;

/// Defines a handler which can take a `Message`.
#[allow(dead_code)]
pub type Handler = Rc<dyn Fn(&Message)>;

// Represents a subscription to a particular topic.
//
// This is an internal class intended to be used by the message bus to organize
// topics and their subscribers.
#[derive(Copy, Clone, Debug)]
pub struct Subscription<T>
where
    T: Clone,
{
    topic: Ustr,
    handler: T,
    handler_id: Ustr,
    priority: u8,
}

impl<T> Subscription<T>
where
    T: Clone,
{
    pub fn new(topic: Ustr, handler: T, handler_id: Ustr, priority: Option<u8>) -> Self {
        Self {
            topic,
            handler,
            handler_id,
            priority: priority.unwrap_or(0),
        }
    }
}

impl<T> PartialEq<Self> for Subscription<T>
where
    T: Clone,
{
    fn eq(&self, other: &Self) -> bool {
        self.topic == other.topic && self.handler_id == other.handler_id
    }
}

impl<T> Eq for Subscription<T> where T: Clone {}

impl<T> PartialOrd for Subscription<T>
where
    T: Clone,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Subscription<T>
where
    T: Clone,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl<T> Hash for Subscription<T>
where
    T: Clone,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.topic.hash(state);
        self.handler_id.hash(state);
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
pub struct MessageBus<T>
where
    T: Clone,
{
    /// The trader ID for the message bus.
    pub trader_id: TraderId,
    /// The name for the message bus.
    pub name: String,
    /// mapping from topic to the corresponding handler
    /// a topic can be a string with wildcards
    /// * '?' - any character
    /// * '*' - any number of any characters
    subscriptions: HashMap<Subscription<T>, Vec<Ustr>>,
    /// maps a pattern to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    patterns: HashMap<Ustr, Vec<Subscription<T>>>,
    /// handles a message or a request destined for a specific endpoint.
    endpoints: HashMap<Ustr, T>,
    /// Relates a request with a response
    /// a request maps it's id to a handler so that a response
    /// with the same id can later be handled.
    correlation_index: HashMap<UUID4, T>,
}

#[allow(dead_code)]
impl<T> MessageBus<T>
where
    T: Clone,
{
    /// Initializes a new instance of the [`MessageBus<T>`].
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
    pub fn endpoints(&self) -> Vec<&str> {
        self.endpoints.keys().map(|k| k.as_str()).collect()
    }

    /// Returns the topics for active subscriptions.
    pub fn topics(&self) -> Vec<&str> {
        self.subscriptions
            .keys()
            .map(|s| s.topic.as_str())
            .collect()
    }

    /// Registers the given `handler` for the `endpoint` address.
    pub fn register(&mut self, endpoint: String, handler: T) {
        // updates value if key already exists
        self.endpoints.insert(Ustr::from(&endpoint), handler);
    }

    /// Deregisters the given `handler` for the `endpoint` address.
    pub fn deregister(&mut self, endpoint: &str) {
        // removes entry if it exists for endpoint
        self.endpoints.remove(&Ustr::from(endpoint));
    }

    /// Subscribes the given `handler` to the `topic`.
    pub fn subscribe(&mut self, topic: &str, handler: T, handler_id: &str, priority: Option<u8>) {
        let sub = Subscription::new(Ustr::from(topic), handler, Ustr::from(handler_id), priority);

        if self.subscriptions.contains_key(&sub) {
            // TODO: log
            return;
        }

        // Find existing patterns which match this topic
        let mut matches = Vec::new();
        for (pattern, subs) in self.patterns.iter_mut() {
            if is_matching(&Ustr::from(topic), pattern) {
                subs.push(sub.clone());
                subs.sort(); // Sort in priority order
                matches.push(*pattern);
            }
        }

        self.subscriptions.insert(sub, matches);
    }

    /// Unsubscribes the given `handler` from the `topic`.
    pub fn unsubscribe(&mut self, topic: &str, handler: T, handler_id: &str) {
        let sub = Subscription::new(Ustr::from(topic), handler, Ustr::from(handler_id), None);

        self.subscriptions.remove(&sub);
    }

    /// Returns the handler for the given `endpoint`.
    pub fn get_endpoint(&self, endpoint: &str) -> Option<&T> {
        self.endpoints.get(&Ustr::from(endpoint))
    }

    /// Returns whether there are subscribers for the given `pattern`.
    pub fn has_subscribers(&self, pattern: &str) -> bool {
        self.matching_handlers(&Ustr::from(pattern))
            .next()
            .is_some()
    }

    // fn send(&self, endpoint: &String, msg: &Message) {
    //     if let Some(handler) = self.endpoints.get(endpoint) {
    //         handler(msg);
    //     }
    // }

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

    // TODO: This is the modified version of matching_subscriptions
    // Since we've separated subscription and handler we can choose to return
    // one of those fields or reconstruct the subscription as a tuple and
    // return that.
    // Depends on on how the output of this function is meant to be used
    fn matching_handlers<'a>(&'a self, pattern: &'a Ustr) -> impl Iterator<Item = &'a T> {
        self.subscriptions.iter().filter_map(move |(sub, _)| {
            if is_matching(&sub.topic, pattern) {
                Some(&sub.handler)
            } else {
                None
            }
        })
    }

    // TODO: Need to improve the efficiency of this
    pub fn get_matching_handlers<'a>(
        &'a mut self,
        pattern: &'a Ustr,
    ) -> &'a mut Vec<Subscription<T>> {
        // The closure must return Vec<Subscription<T>>, not Vec<T>
        let matching_handlers = || {
            self.subscriptions
                .iter()
                .filter_map(|(sub, _)| {
                    if is_matching(&sub.topic, pattern) {
                        Some(sub.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<Subscription<T>>>()
        };

        self.patterns
            .entry(*pattern)
            .or_insert_with(matching_handlers)
    }

    pub fn publish(&mut self, pattern: Ustr, _msg: &Message) {
        let _handlers = self.get_matching_handlers(&pattern);

        // call matched handlers
        // handlers.iter().for_each(|handler| handler(msg));
    }
}

/// match a topic and a string pattern
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
fn is_matching(topic: &Ustr, pattern: &Ustr) -> bool {
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
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_new() {
        let trader_id = TraderId::from("trader-001");
        let msgbus = MessageBus::<Handler>::new(trader_id, None);

        assert_eq!(msgbus.trader_id, trader_id);
        assert_eq!(msgbus.name, stringify!(MessageBus));
    }

    #[rstest]
    fn test_endpoints_when_no_endpoints() {
        let msgbus = MessageBus::<Handler>::new(TraderId::from("trader-001"), None);

        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_topics_when_no_subscriptions() {
        let msgbus = MessageBus::<Handler>::new(TraderId::from("trader-001"), None);

        assert!(msgbus.topics().is_empty());
        assert!(!msgbus.has_subscribers(&"my-topic".to_string()));
    }

    #[rstest]
    fn test_regsiter_endpoint() {
        let mut msgbus = MessageBus::<Handler>::new(TraderId::from("trader-001"), None);
        let endpoint = "MyEndpoint".to_string();

        // Useless handler for testing
        let handler = Rc::new(|m: &_| {
            format!("{:?}", m);
        });

        msgbus.register(endpoint.clone(), handler.clone());

        assert_eq!(msgbus.endpoints(), vec!["MyEndpoint".to_string()]);
        assert!(msgbus.get_endpoint(&endpoint).is_some());
    }

    #[rstest]
    fn test_deregsiter_endpoint() {
        let mut msgbus = MessageBus::<Handler>::new(TraderId::from("trader-001"), None);
        let endpoint = "MyEndpoint".to_string();

        // Useless handler for testing
        let handler = Rc::new(|m: &_| {
            format!("{:?}", m);
        });

        msgbus.register(endpoint.clone(), handler.clone());
        msgbus.deregister(&endpoint);

        assert!(msgbus.endpoints().is_empty());
    }

    #[rstest]
    fn test_subscribe() {
        let mut msgbus = MessageBus::<Handler>::new(TraderId::from("trader-001"), None);
        let topic = "my-topic".to_string();

        // Useless handler for testing
        let handler = Rc::new(|m: &_| {
            format!("{:?}", m);
        });

        msgbus.subscribe(&topic, handler.clone(), "a", Some(1));

        assert!(msgbus.has_subscribers(&topic));
        assert_eq!(msgbus.topics(), vec![topic]);
    }

    #[rstest]
    fn test_unsubscribe() {
        let mut msgbus = MessageBus::<Handler>::new(TraderId::from("trader-001"), None);
        let topic = "my-topic".to_string();

        // Useless handler for testing
        let handler = Rc::new(|m: &_| {
            format!("{:?}", m);
        });

        msgbus.subscribe(&topic, handler.clone(), "a", None);
        msgbus.unsubscribe(&topic, handler.clone(), "a");

        assert!(msgbus.topics().is_empty());
    }
}

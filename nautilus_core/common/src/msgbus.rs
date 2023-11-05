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

use std::collections::HashMap;

use nautilus_core::{message::Message, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;

// Previous handler alias for Rust
// type Handler = Rc<dyn Fn(&Message)>;

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
    subscriptions: HashMap<String, T>,
    /// maps a pattern to all the handlers registered for it
    /// this is updated whenever a new subscription is created.
    patterns: HashMap<String, Vec<T>>,
    /// handles a message or a request destined for a specific endpoint.
    endpoints: HashMap<String, T>,
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

    pub fn endpoints(&self) -> Vec<&str> {
        self.endpoints.keys().map(|k| k.as_str()).collect()
    }

    pub fn topics(&self) -> Vec<&str> {
        self.subscriptions.keys().map(|k| k.as_str()).collect()
    }

    pub fn register(&mut self, endpoint: String, handler: T) {
        // updates value if key already exists
        self.endpoints.insert(endpoint, handler);
    }

    pub fn deregister(&mut self, endpoint: &String) {
        // removes entry if it exists for endpoint
        self.endpoints.remove(endpoint);
    }

    pub fn subscribe(&mut self, topic: String, handler: T) {
        if self.subscriptions.contains_key(&topic) {
            // TODO: log
            return;
        }

        self.subscriptions.insert(topic, handler);
    }

    #[allow(unused_variables)]
    pub fn unsubscribe(&mut self, topic: &String, handler: T) {
        self.subscriptions.remove(topic);
    }

    pub fn get_endpoint(&self, endpoint: &str) -> Option<&T> {
        self.endpoints.get(endpoint)
    }

    pub fn has_subscribers(&self, pattern: &String) -> bool {
        self.matching_handlers(pattern).next().is_some()
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
    fn matching_handlers<'a>(&'a self, pattern: &'a String) -> impl Iterator<Item = &'a T> {
        self.subscriptions.iter().filter_map(|(topic, handler)| {
            if is_matching(topic, pattern) {
                Some(handler)
            } else {
                None
            }
        })
    }

    pub fn get_matching_handlers(&mut self, pattern: String) -> &mut Vec<T> {
        // TODO: check if clone can be avoided
        // Although not possible with this style - https://github.com/rust-lang/rust/issues/51604
        let entry = self.patterns.entry(pattern.clone());

        let matching_handlers = || {
            self.subscriptions
                .iter()
                .filter_map(|(topic, handler)| {
                    if is_matching(topic, &pattern) {
                        Some(handler.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        entry.or_insert_with(matching_handlers)
    }

    fn publish(&mut self, pattern: String, _msg: &Message) {
        let _handlers = self.get_matching_handlers(pattern);

        // call matched handlers
        // handlers.iter().for_each(|handler| handler(msg));
    }
}

/// match a topic and a string pattern
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
fn is_matching(topic: &String, pattern: &String) -> bool {
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

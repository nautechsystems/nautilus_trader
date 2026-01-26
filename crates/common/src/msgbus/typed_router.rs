// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Type-safe topic routing for pub/sub messaging.
//!
//! This module provides [`TopicRouter<T>`] for routing messages of a specific type
//! to subscribed handlers based on topic patterns.

use std::{
    cmp::Ordering,
    fmt::Debug,
    hash::{Hash, Hasher},
};

use indexmap::IndexMap;
use smallvec::SmallVec;
use ustr::Ustr;

use super::{
    matching::is_matching_backtracking,
    mstr::{MStr, Pattern, Topic},
    typed_handler::TypedHandler,
};

/// A typed subscription for pub/sub messaging.
///
/// Associates a handler with a topic pattern and priority.
#[derive(Clone)]
pub struct TypedSubscription<T: 'static> {
    /// The typed message handler.
    pub handler: TypedHandler<T>,
    /// Cached handler ID for faster equality checks.
    pub handler_id: Ustr,
    /// The pattern for matching topics.
    pub pattern: MStr<Pattern>,
    /// Higher priority handlers receive messages first.
    pub priority: u8,
}

impl<T: 'static> TypedSubscription<T> {
    /// Creates a new typed subscription.
    #[must_use]
    pub fn new(pattern: MStr<Pattern>, handler: TypedHandler<T>, priority: Option<u8>) -> Self {
        Self {
            handler_id: handler.id(),
            pattern,
            handler,
            priority: priority.unwrap_or(0),
        }
    }
}

impl<T: 'static> Debug for TypedSubscription<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TypedSubscription))
            .field("handler_id", &self.handler_id)
            .field("pattern", &self.pattern)
            .field("priority", &self.priority)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: 'static> PartialEq for TypedSubscription<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern && self.handler_id == other.handler_id
    }
}

impl<T: 'static> Eq for TypedSubscription<T> {}

impl<T: 'static> PartialOrd for TypedSubscription<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: 'static> Ord for TypedSubscription<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first (descending)
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.pattern.cmp(&other.pattern))
            .then_with(|| self.handler_id.cmp(&other.handler_id))
    }
}

impl<T: 'static> Hash for TypedSubscription<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pattern.hash(state);
        self.handler_id.hash(state);
    }
}

/// Routes messages of type `T` to subscribed handlers based on topic patterns.
///
/// Supports wildcard patterns (`*` and `?`) and priority-based ordering.
/// Caches topic-to-subscription mappings for efficient repeated lookups.
#[derive(Debug)]
pub struct TopicRouter<T: 'static> {
    /// All active subscriptions.
    pub(crate) subscriptions: Vec<TypedSubscription<T>>,
    /// Cache mapping topics to matching subscription indices (inline for ≤64 handlers).
    topic_cache: IndexMap<MStr<Topic>, SmallVec<[usize; 64]>>,
}

impl<T: 'static> Default for TopicRouter<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> TopicRouter<T> {
    /// Creates a new empty topic router.
    #[must_use]
    pub fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
            topic_cache: IndexMap::new(),
        }
    }

    /// Returns the number of active subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns whether there are any subscriptions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// Returns all active subscription patterns.
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.pattern.as_str())
            .collect()
    }

    /// Returns all subscription handler IDs.
    #[must_use]
    pub fn handler_ids(&self) -> Vec<&str> {
        self.subscriptions
            .iter()
            .map(|s| s.handler_id.as_str())
            .collect()
    }

    /// Subscribes a handler to a topic pattern.
    ///
    /// # Warning
    ///
    /// Assigning priority is an advanced feature. Higher priority handlers
    /// receive messages before lower priority handlers.
    pub fn subscribe(&mut self, pattern: MStr<Pattern>, handler: TypedHandler<T>, priority: u8) {
        let sub = TypedSubscription::new(pattern, handler, Some(priority));

        // Check for duplicate
        if self.subscriptions.iter().any(|s| s == &sub) {
            log::warn!("{sub:?} already exists");
            return;
        }

        log::debug!("Subscribing {sub:?}");

        // Invalidate cache entries that match this pattern
        self.invalidate_cache_for_pattern(pattern);

        self.subscriptions.push(sub);

        // Re-sort by priority (descending)
        self.subscriptions.sort();
    }

    /// Unsubscribes a handler from a topic pattern.
    pub fn unsubscribe(&mut self, pattern: MStr<Pattern>, handler: &TypedHandler<T>) {
        log::debug!(
            "Unsubscribing handler {} from pattern '{pattern}'",
            handler.id()
        );

        let handler_id = handler.id();
        if let Some(idx) = self
            .subscriptions
            .iter()
            .position(|s| s.pattern == pattern && s.handler_id == handler_id)
        {
            self.subscriptions.remove(idx);
            self.invalidate_cache_for_pattern(pattern);
            log::debug!("Handler for pattern '{pattern}' was removed");
        } else {
            log::debug!("No matching handler for pattern '{pattern}' was found");
        }
    }

    /// Checks if a handler is subscribed to a pattern.
    #[must_use]
    pub fn is_subscribed(&self, pattern: MStr<Pattern>, handler: &TypedHandler<T>) -> bool {
        let handler_id = handler.id();
        self.subscriptions
            .iter()
            .any(|s| s.pattern == pattern && s.handler_id == handler_id)
    }

    /// Returns whether there are subscribers for the topic.
    #[must_use]
    pub fn has_subscribers(&self, topic: MStr<Topic>) -> bool {
        self.get_matching_indices(topic).map_or_else(
            || !self.find_matches(topic).is_empty(),
            |indices| !indices.is_empty(),
        )
    }

    /// Returns the count of subscribers for a topic.
    #[must_use]
    pub fn subscriber_count(&self, topic: MStr<Topic>) -> usize {
        self.get_matching_indices(topic)
            .map_or_else(|| self.find_matches(topic).len(), |indices| indices.len())
    }

    /// Publishes a message to all handlers subscribed to matching patterns.
    pub fn publish(&mut self, topic: MStr<Topic>, message: &T) {
        // Split borrow to avoid copying indices
        let Self {
            subscriptions,
            topic_cache,
        } = self;

        let indices = topic_cache.entry(topic).or_insert_with(|| {
            subscriptions
                .iter()
                .enumerate()
                .filter_map(|(idx, sub)| {
                    if is_matching_backtracking(topic, sub.pattern) {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect()
        });

        for &idx in indices.iter() {
            subscriptions[idx].handler.handle(message);
        }
    }

    /// Returns cloned handlers matching a topic for safe out-of-borrow calling.
    ///
    /// Use this when handlers may need to access the message bus during execution.
    /// Note: Allocates a Vec on each call. For hot paths, prefer the thread-local
    /// buffer pattern used by `publish_*` functions.
    pub fn get_matching_handlers(&mut self, topic: MStr<Topic>) -> Vec<TypedHandler<T>> {
        let indices: SmallVec<[usize; 64]> = self
            .get_or_compute_matching_indices(topic)
            .iter()
            .copied()
            .collect();
        indices
            .into_iter()
            .map(|idx| self.subscriptions[idx].handler.clone())
            .collect()
    }

    /// Gets cached matching indices for a topic, if available.
    fn get_matching_indices(&self, topic: MStr<Topic>) -> Option<&[usize]> {
        self.topic_cache.get(&topic).map(|v| v.as_slice())
    }

    /// Gets or computes matching subscription indices for a topic.
    pub(crate) fn get_or_compute_matching_indices(&mut self, topic: MStr<Topic>) -> &[usize] {
        if !self.topic_cache.contains_key(&topic) {
            let indices = self.find_matches(topic);
            self.topic_cache.insert(topic, indices);
        }
        self.topic_cache.get(&topic).unwrap()
    }

    /// Fills a buffer with handlers matching a topic.
    pub(crate) fn fill_matching_handlers(
        &mut self,
        topic: MStr<Topic>,
        buf: &mut SmallVec<[TypedHandler<T>; 64]>,
    ) {
        let Self {
            subscriptions,
            topic_cache,
        } = self;

        let indices = topic_cache.entry(topic).or_insert_with(|| {
            subscriptions
                .iter()
                .enumerate()
                .filter_map(|(idx, sub)| {
                    if is_matching_backtracking(topic, sub.pattern) {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect()
        });

        for &idx in indices.iter() {
            buf.push(subscriptions[idx].handler.clone());
        }
    }

    /// Finds subscription indices matching a topic (without caching).
    fn find_matches(&self, topic: MStr<Topic>) -> SmallVec<[usize; 64]> {
        self.subscriptions
            .iter()
            .enumerate()
            .filter_map(|(idx, sub)| {
                if is_matching_backtracking(topic, sub.pattern) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Invalidates cache entries that could be affected by a pattern change.
    fn invalidate_cache_for_pattern(&mut self, pattern: MStr<Pattern>) {
        // Remove cached entries where the pattern might match the topic
        self.topic_cache
            .retain(|topic, _| !is_matching_backtracking(*topic, pattern));
    }

    /// Clears all subscriptions and cache.
    pub fn clear(&mut self) {
        self.subscriptions.clear();
        self.topic_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_topic_router_subscribe_and_publish() {
        let mut router = TopicRouter::<String>::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |msg: &String| {
            received_clone.borrow_mut().push(msg.clone());
        });

        router.subscribe("data.quotes.*".into(), handler, 0);

        let topic: MStr<Topic> = "data.quotes.AAPL".into();
        router.publish(topic, &"quote1".to_string());
        router.publish(topic, &"quote2".to_string());

        assert_eq!(*received.borrow(), vec!["quote1", "quote2"]);
    }

    #[rstest]
    fn test_topic_router_priority_ordering() {
        let mut router = TopicRouter::<i32>::new();
        let order = Rc::new(RefCell::new(Vec::new()));

        let order1 = order.clone();
        let handler1 = TypedHandler::from_with_id("low", move |_: &i32| {
            order1.borrow_mut().push("low");
        });

        let order2 = order.clone();
        let handler2 = TypedHandler::from_with_id("high", move |_: &i32| {
            order2.borrow_mut().push("high");
        });

        // Subscribe low priority first, high priority second
        router.subscribe("test.*".into(), handler1, 5);
        router.subscribe("test.*".into(), handler2, 10);

        let topic: MStr<Topic> = "test.topic".into();
        router.publish(topic, &42);

        // High priority should be called first
        assert_eq!(*order.borrow(), vec!["high", "low"]);
    }

    #[rstest]
    fn test_topic_router_unsubscribe() {
        let mut router = TopicRouter::<String>::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from_with_id("test-handler", move |msg: &String| {
            received_clone.borrow_mut().push(msg.clone());
        });

        router.subscribe("data.*".into(), handler.clone(), 0);
        assert!(router.is_subscribed("data.*".into(), &handler));

        router.unsubscribe("data.*".into(), &handler);
        assert!(!router.is_subscribed("data.*".into(), &handler));

        let topic: MStr<Topic> = "data.test".into();
        router.publish(topic, &"test".to_string());

        // Should not receive anything after unsubscribe
        assert!(received.borrow().is_empty());
    }

    #[rstest]
    fn test_topic_router_duplicate_subscription() {
        let mut router = TopicRouter::<i32>::new();

        let handler1 = TypedHandler::from_with_id("dup-handler", |_: &i32| {});
        let handler2 = TypedHandler::from_with_id("dup-handler", |_: &i32| {});

        router.subscribe("test.*".into(), handler1, 0);
        router.subscribe("test.*".into(), handler2, 0);

        // Should only have one subscription
        assert_eq!(router.subscription_count(), 1);
    }

    #[rstest]
    fn test_topic_router_wildcard_patterns() {
        let mut router = TopicRouter::<String>::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |msg: &String| {
            received_clone.borrow_mut().push(msg.clone());
        });

        router.subscribe("data.*.AAPL".into(), handler, 0);

        // Should match
        let topic1: MStr<Topic> = "data.quotes.AAPL".into();
        router.publish(topic1, &"match1".to_string());

        let topic2: MStr<Topic> = "data.trades.AAPL".into();
        router.publish(topic2, &"match2".to_string());

        // Should not match
        let topic3: MStr<Topic> = "data.quotes.MSFT".into();
        router.publish(topic3, &"no-match".to_string());

        assert_eq!(*received.borrow(), vec!["match1", "match2"]);
    }

    #[rstest]
    fn test_topic_router_cache_populated_on_publish() {
        let mut router = TopicRouter::<i32>::new();
        let handler = TypedHandler::from_with_id("cache-test", |_: &i32| {});

        router.subscribe("data.*".into(), handler, 0);

        // First publish populates cache
        let topic: MStr<Topic> = "data.quotes".into();
        router.publish(topic, &1);

        // Verify cache is used (subscriber_count uses cache if available)
        assert_eq!(router.subscriber_count(topic), 1);
    }

    #[rstest]
    fn test_topic_router_cache_invalidated_on_subscribe() {
        let mut router = TopicRouter::<i32>::new();
        let received = Rc::new(RefCell::new(0));

        let r1 = received.clone();
        let handler1 = TypedHandler::from_with_id("h1", move |_: &i32| {
            *r1.borrow_mut() += 1;
        });

        router.subscribe("data.*".into(), handler1, 0);

        // Publish to populate cache
        let topic: MStr<Topic> = "data.test".into();
        router.publish(topic, &1);
        assert_eq!(*received.borrow(), 1);

        // Subscribe new handler (should invalidate cache)
        let r2 = received.clone();
        let handler2 = TypedHandler::from_with_id("h2", move |_: &i32| {
            *r2.borrow_mut() += 10;
        });
        router.subscribe("data.*".into(), handler2, 0);

        // Publish again - both handlers should receive
        router.publish(topic, &2);
        assert_eq!(*received.borrow(), 12); // 1 + 1 + 10
    }

    #[rstest]
    fn test_topic_router_cache_invalidated_on_unsubscribe() {
        let mut router = TopicRouter::<i32>::new();
        let received = Rc::new(RefCell::new(0));

        let r1 = received.clone();
        let handler1 = TypedHandler::from_with_id("h1", move |_: &i32| {
            *r1.borrow_mut() += 1;
        });

        let r2 = received.clone();
        let handler2 = TypedHandler::from_with_id("h2", move |_: &i32| {
            *r2.borrow_mut() += 10;
        });

        router.subscribe("data.*".into(), handler1.clone(), 0);
        router.subscribe("data.*".into(), handler2, 0);

        // Publish to populate cache
        let topic: MStr<Topic> = "data.test".into();
        router.publish(topic, &1);
        assert_eq!(*received.borrow(), 11); // 1 + 10

        // Unsubscribe handler1 (should invalidate cache)
        router.unsubscribe("data.*".into(), &handler1);

        // Publish again - only handler2 should receive
        router.publish(topic, &2);
        assert_eq!(*received.borrow(), 21); // 11 + 10
    }

    #[rstest]
    fn test_topic_router_has_subscribers() {
        let mut router = TopicRouter::<i32>::new();

        let topic: MStr<Topic> = "data.quotes.AAPL".into();
        assert!(!router.has_subscribers(topic));

        let handler = TypedHandler::from_with_id("test", |_: &i32| {});
        router.subscribe("data.quotes.*".into(), handler, 0);

        assert!(router.has_subscribers(topic));
    }

    #[rstest]
    fn test_topic_router_subscriber_count() {
        let mut router = TopicRouter::<i32>::new();

        let topic: MStr<Topic> = "data.quotes.AAPL".into();
        assert_eq!(router.subscriber_count(topic), 0);

        let handler1 = TypedHandler::from_with_id("h1", |_: &i32| {});
        let handler2 = TypedHandler::from_with_id("h2", |_: &i32| {});
        let handler3 = TypedHandler::from_with_id("h3", |_: &i32| {});

        router.subscribe("data.quotes.*".into(), handler1, 0);
        router.subscribe("data.*.AAPL".into(), handler2, 0);
        router.subscribe("events.*".into(), handler3, 0); // Won't match

        assert_eq!(router.subscriber_count(topic), 2);
    }

    #[rstest]
    fn test_topic_router_patterns_and_handler_ids() {
        let mut router = TopicRouter::<i32>::new();

        let handler1 = TypedHandler::from_with_id("handler-a", |_: &i32| {});
        let handler2 = TypedHandler::from_with_id("handler-b", |_: &i32| {});

        router.subscribe("pattern.one".into(), handler1, 0);
        router.subscribe("pattern.two".into(), handler2, 0);

        let patterns = router.patterns();
        assert!(patterns.contains(&"pattern.one"));
        assert!(patterns.contains(&"pattern.two"));

        let ids = router.handler_ids();
        assert!(ids.contains(&"handler-a"));
        assert!(ids.contains(&"handler-b"));
    }

    #[rstest]
    fn test_topic_router_clear() {
        let mut router = TopicRouter::<i32>::new();
        let handler = TypedHandler::from_with_id("clear-test", |_: &i32| {});

        router.subscribe("data.*".into(), handler, 0);

        // Populate cache
        let topic: MStr<Topic> = "data.test".into();
        router.publish(topic, &1);

        assert_eq!(router.subscription_count(), 1);
        assert!(!router.is_empty());

        router.clear();

        assert_eq!(router.subscription_count(), 0);
        assert!(router.is_empty());
        assert!(!router.has_subscribers(topic));
    }

    #[rstest]
    fn test_topic_router_multiple_patterns_same_topic() {
        let mut router = TopicRouter::<i32>::new();
        let received = Rc::new(RefCell::new(Vec::new()));

        let r1 = received.clone();
        let handler1 = TypedHandler::from_with_id("specific", move |v: &i32| {
            r1.borrow_mut().push(format!("specific:{v}"));
        });

        let r2 = received.clone();
        let handler2 = TypedHandler::from_with_id("wildcard", move |v: &i32| {
            r2.borrow_mut().push(format!("wildcard:{v}"));
        });

        let r3 = received.clone();
        let handler3 = TypedHandler::from_with_id("all", move |v: &i32| {
            r3.borrow_mut().push(format!("all:{v}"));
        });

        // All three patterns match "data.quotes.AAPL"
        router.subscribe("data.quotes.AAPL".into(), handler1, 0);
        router.subscribe("data.quotes.*".into(), handler2, 0);
        router.subscribe("data.*.*".into(), handler3, 0);

        let topic: MStr<Topic> = "data.quotes.AAPL".into();
        router.publish(topic, &42);

        let msgs = received.borrow();
        assert_eq!(msgs.len(), 3);
        assert!(msgs.contains(&"specific:42".to_string()));
        assert!(msgs.contains(&"wildcard:42".to_string()));
        assert!(msgs.contains(&"all:42".to_string()));
    }
}

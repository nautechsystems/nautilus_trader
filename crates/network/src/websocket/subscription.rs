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

//! Generic subscription state tracking for WebSocket clients.
//!
//! This module provides a robust subscription tracker that maintains confirmed and pending
//! subscription states with reference counting support. It follows a proven pattern used in
//! production exchange adapters.
//!
//! # Key Features
//!
//! - **Three-state tracking**: confirmed, pending_subscribe, pending_unsubscribe.
//! - **Reference counting**: Prevents duplicate subscribe/unsubscribe messages.
//! - **Reconnection support**: `all_topics()` returns topics to resubscribe after reconnect.
//! - **Configurable delimiter**: Supports different topic formats (`.` or `:` etc.).
//!
//! # Topic Format
//!
//! Topics are strings in the format `channel{delimiter}symbol`:
//! - Bybit: `tickers.BTCUSDT` (delimiter: `.`)
//! - BitMEX: `orderBookL2:XBTUSD` (delimiter: `:`)
//! - OKX: `trades:BTC-USDT` (delimiter: `:`)
//!
//! Channels without symbols are also supported (e.g., `orderbook` for all instruments).

use std::{num::NonZeroUsize, sync::Arc};

use ahash::AHashSet;
use dashmap::DashMap;
use ustr::Ustr;

/// Marker for channel-level subscriptions (no specific symbol).
///
/// An empty string in the symbol set indicates a channel-level subscription
/// that applies to all symbols for that channel.
const CHANNEL_LEVEL_MARKER: &str = "";

/// Splits a topic into channel and optional symbol using the specified delimiter.
pub fn split_topic(topic: &str, delimiter: char) -> (&str, Option<&str>) {
    topic
        .split_once(delimiter)
        .map_or((topic, None), |(channel, symbol)| (channel, Some(symbol)))
}

/// Tracks a topic in the given map by adding it to the channel's symbol set.
///
/// Channel-level subscriptions are stored using an empty string marker,
/// allowing both channel-level and symbol-level subscriptions to coexist.
fn track_topic(map: &DashMap<Ustr, AHashSet<Ustr>>, channel: &str, symbol: Option<&str>) {
    let channel_ustr = Ustr::from(channel);
    let mut entry = map.entry(channel_ustr).or_default();

    if let Some(symbol) = symbol {
        entry.insert(Ustr::from(symbol));
    } else {
        entry.insert(Ustr::from(CHANNEL_LEVEL_MARKER));
    }
}

/// Removes a topic from the given map by removing it from the channel's symbol set.
///
/// Removes the entire channel entry if no subscriptions remain after removal.
fn untrack_topic(map: &DashMap<Ustr, AHashSet<Ustr>>, channel: &str, symbol: Option<&str>) {
    let channel_ustr = Ustr::from(channel);
    let symbol_to_remove = if let Some(symbol) = symbol {
        Ustr::from(symbol)
    } else {
        Ustr::from(CHANNEL_LEVEL_MARKER)
    };

    let mut remove_channel = false;
    if let Some(mut entry) = map.get_mut(&channel_ustr) {
        entry.remove(&symbol_to_remove);
        remove_channel = entry.is_empty();
    }

    if remove_channel {
        map.remove(&channel_ustr);
    }
}

/// Checks if a topic exists in the given map.
fn is_tracked(map: &DashMap<Ustr, AHashSet<Ustr>>, channel: &str, symbol: Option<&str>) -> bool {
    let channel_ustr = Ustr::from(channel);
    let symbol_to_check = if let Some(symbol) = symbol {
        Ustr::from(symbol)
    } else {
        Ustr::from(CHANNEL_LEVEL_MARKER)
    };

    if let Some(entry) = map.get(&channel_ustr) {
        entry.contains(&symbol_to_check)
    } else {
        false
    }
}

/// Generic subscription state tracker for WebSocket connections.
///
/// Maintains three separate states for subscriptions:
/// - **Confirmed**: Successfully subscribed and actively streaming data.
/// - **Pending Subscribe**: Subscription requested but not yet confirmed by server.
/// - **Pending Unsubscribe**: Unsubscription requested but not yet confirmed by server.
///
/// # Reference Counting
///
/// The tracker maintains reference counts for each topic. When multiple components
/// subscribe to the same topic, only the first subscription sends a message to the
/// server. Similarly, only the last unsubscription sends an unsubscribe message.
///
/// # Thread Safety
///
/// All operations are thread-safe and can be called concurrently from multiple tasks.
#[derive(Clone, Debug)]
pub struct SubscriptionState {
    /// Confirmed active subscriptions.
    confirmed: Arc<DashMap<Ustr, AHashSet<Ustr>>>,
    /// Pending subscribe requests awaiting server confirmation.
    pending_subscribe: Arc<DashMap<Ustr, AHashSet<Ustr>>>,
    /// Pending unsubscribe requests awaiting server confirmation.
    pending_unsubscribe: Arc<DashMap<Ustr, AHashSet<Ustr>>>,
    /// Reference counts for topics to prevent duplicate messages.
    reference_counts: Arc<DashMap<Ustr, NonZeroUsize>>,
    /// Topic delimiter character (e.g., '.' or ':').
    delimiter: char,
}

impl SubscriptionState {
    /// Creates a new subscription state tracker with the specified topic delimiter.
    pub fn new(delimiter: char) -> Self {
        Self {
            confirmed: Arc::new(DashMap::new()),
            pending_subscribe: Arc::new(DashMap::new()),
            pending_unsubscribe: Arc::new(DashMap::new()),
            reference_counts: Arc::new(DashMap::new()),
            delimiter,
        }
    }

    /// Returns the delimiter character used for topic splitting.
    pub fn delimiter(&self) -> char {
        self.delimiter
    }

    /// Returns a clone of the confirmed subscriptions map.
    pub fn confirmed(&self) -> Arc<DashMap<Ustr, AHashSet<Ustr>>> {
        Arc::clone(&self.confirmed)
    }

    /// Returns a clone of the pending subscribe map.
    pub fn pending_subscribe(&self) -> Arc<DashMap<Ustr, AHashSet<Ustr>>> {
        Arc::clone(&self.pending_subscribe)
    }

    /// Returns a clone of the pending unsubscribe map.
    pub fn pending_unsubscribe(&self) -> Arc<DashMap<Ustr, AHashSet<Ustr>>> {
        Arc::clone(&self.pending_unsubscribe)
    }

    /// Returns the number of confirmed subscriptions.
    ///
    /// Counts both channel-level and symbol-level subscriptions.
    pub fn len(&self) -> usize {
        self.confirmed.iter().map(|entry| entry.value().len()).sum()
    }

    /// Returns true if there are no subscriptions (confirmed or pending).
    pub fn is_empty(&self) -> bool {
        self.confirmed.is_empty()
            && self.pending_subscribe.is_empty()
            && self.pending_unsubscribe.is_empty()
    }

    /// Marks a topic as pending subscription.
    ///
    /// This should be called after sending a subscribe request to the server.
    pub fn mark_subscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);
        track_topic(&self.pending_subscribe, channel, symbol);
    }

    /// Marks a topic as pending unsubscription.
    ///
    /// This removes the topic from both confirmed and pending_subscribe,
    /// then adds it to pending_unsubscribe. This handles the case where
    /// a user unsubscribes before the initial subscription is confirmed.
    pub fn mark_unsubscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);
        track_topic(&self.pending_unsubscribe, channel, symbol);
        untrack_topic(&self.confirmed, channel, symbol);
        untrack_topic(&self.pending_subscribe, channel, symbol);
    }

    /// Confirms a subscription by moving it from pending to confirmed.
    ///
    /// This should be called when the server acknowledges a subscribe request.
    /// Ignores the confirmation if the topic is pending unsubscription (handles
    /// late confirmations after user has already unsubscribed).
    pub fn confirm_subscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);

        // Ignore late confirmations if topic is pending unsubscribe
        if is_tracked(&self.pending_unsubscribe, channel, symbol) {
            return;
        }

        untrack_topic(&self.pending_subscribe, channel, symbol);
        track_topic(&self.confirmed, channel, symbol);
    }

    /// Confirms an unsubscription by removing it from all state maps.
    ///
    /// This should be called when the server acknowledges an unsubscribe request.
    /// Removes the topic from pending_unsubscribe, confirmed, and pending_subscribe
    /// to handle race conditions where a late subscribe confirmation arrives.
    pub fn confirm_unsubscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);
        untrack_topic(&self.pending_unsubscribe, channel, symbol);
        untrack_topic(&self.confirmed, channel, symbol);
        untrack_topic(&self.pending_subscribe, channel, symbol);
    }

    /// Marks a subscription as failed, moving it from confirmed back to pending.
    ///
    /// This is useful when a subscription fails but should be retried on reconnect.
    /// Ignores the failure if the topic is pending unsubscription (user cancelled it).
    pub fn mark_failure(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);

        // Ignore failures for topics being unsubscribed
        if is_tracked(&self.pending_unsubscribe, channel, symbol) {
            return;
        }

        untrack_topic(&self.confirmed, channel, symbol);
        track_topic(&self.pending_subscribe, channel, symbol);
    }

    /// Returns all pending subscribe topics as strings.
    pub fn pending_subscribe_topics(&self) -> Vec<String> {
        self.topics_from_map(&self.pending_subscribe)
    }

    /// Returns all pending unsubscribe topics as strings.
    pub fn pending_unsubscribe_topics(&self) -> Vec<String> {
        self.topics_from_map(&self.pending_unsubscribe)
    }

    /// Returns all topics that should be active (confirmed + pending_subscribe).
    ///
    /// This is the key method for reconnection: it returns all topics that should
    /// be resubscribed after a connection is re-established.
    ///
    /// Note: Does NOT include pending_unsubscribe topics, as those are being removed.
    pub fn all_topics(&self) -> Vec<String> {
        let mut topics = Vec::new();
        topics.extend(self.topics_from_map(&self.confirmed));
        topics.extend(self.topics_from_map(&self.pending_subscribe));
        topics
    }

    /// Helper to convert a map to topic strings.
    fn topics_from_map(&self, map: &DashMap<Ustr, AHashSet<Ustr>>) -> Vec<String> {
        let mut topics = Vec::new();
        let marker = Ustr::from(CHANNEL_LEVEL_MARKER);

        for entry in map.iter() {
            let channel = entry.key();
            let symbols = entry.value();

            // Check for channel-level subscription marker
            if symbols.contains(&marker) {
                topics.push(channel.to_string());
            }

            // Add symbol-level subscriptions (skip marker)
            for symbol in symbols.iter() {
                if *symbol != marker {
                    topics.push(format!(
                        "{}{}{}",
                        channel.as_str(),
                        self.delimiter,
                        symbol.as_str()
                    ));
                }
            }
        }

        topics
    }

    /// Increments the reference count for a topic.
    ///
    /// Returns `true` if this is the first subscription (caller should send subscribe
    /// message to server).
    ///
    /// # Panics
    ///
    /// Panics if the reference count exceeds `usize::MAX` subscriptions for a single topic.
    pub fn add_reference(&self, topic: &str) -> bool {
        let mut should_subscribe = false;
        let topic_ustr = Ustr::from(topic);

        self.reference_counts
            .entry(topic_ustr)
            .and_modify(|count| {
                *count = NonZeroUsize::new(count.get() + 1).expect("reference count overflow");
            })
            .or_insert_with(|| {
                should_subscribe = true;
                NonZeroUsize::new(1).expect("NonZeroUsize::new(1) should never fail")
            });

        should_subscribe
    }

    /// Decrements the reference count for a topic.
    ///
    /// Returns `true` if this was the last subscription (caller should send unsubscribe
    /// message to server).
    ///
    /// # Panics
    ///
    /// Panics if the internal reference count state becomes inconsistent (should never happen
    /// if the API is used correctly).
    pub fn remove_reference(&self, topic: &str) -> bool {
        let topic_ustr = Ustr::from(topic);
        if let Some(mut entry) = self.reference_counts.get_mut(&topic_ustr) {
            let current = entry.get();

            if current == 1 {
                drop(entry);
                self.reference_counts.remove(&topic_ustr);
                return true;
            }

            *entry = NonZeroUsize::new(current - 1)
                .expect("reference count should never reach zero here");
        }

        false
    }

    /// Returns the current reference count for a topic.
    ///
    /// Returns 0 if the topic has no references.
    pub fn get_reference_count(&self, topic: &str) -> usize {
        let topic_ustr = Ustr::from(topic);
        self.reference_counts
            .get(&topic_ustr)
            .map_or(0, |count| count.get())
    }

    /// Clears all subscription state.
    ///
    /// This is useful when reconnecting or resetting the client.
    pub fn clear(&self) {
        self.confirmed.clear();
        self.pending_subscribe.clear();
        self.pending_unsubscribe.clear();
        self.reference_counts.clear();
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_split_topic_with_symbol() {
        let (channel, symbol) = split_topic("tickers.BTCUSDT", '.');
        assert_eq!(channel, "tickers");
        assert_eq!(symbol, Some("BTCUSDT"));

        let (channel, symbol) = split_topic("orderBookL2:XBTUSD", ':');
        assert_eq!(channel, "orderBookL2");
        assert_eq!(symbol, Some("XBTUSD"));
    }

    #[rstest]
    fn test_split_topic_without_symbol() {
        let (channel, symbol) = split_topic("orderbook", '.');
        assert_eq!(channel, "orderbook");
        assert_eq!(symbol, None);
    }

    #[rstest]
    fn test_new_state_is_empty() {
        let state = SubscriptionState::new('.');
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);
    }

    #[rstest]
    fn test_mark_subscribe() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");

        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]);
        assert_eq!(state.len(), 0); // Not confirmed yet
    }

    #[rstest]
    fn test_confirm_subscribe() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");

        assert!(state.pending_subscribe_topics().is_empty());
        assert_eq!(state.len(), 1);
    }

    #[rstest]
    fn test_mark_unsubscribe() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.mark_unsubscribe("tickers.BTCUSDT");

        assert_eq!(state.len(), 0); // Removed from confirmed
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);
    }

    #[rstest]
    fn test_confirm_unsubscribe() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.mark_unsubscribe("tickers.BTCUSDT");
        state.confirm_unsubscribe("tickers.BTCUSDT");

        assert!(state.is_empty());
    }

    #[rstest]
    fn test_mark_failure() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.mark_failure("tickers.BTCUSDT");

        assert_eq!(state.len(), 0);
        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]);
    }

    #[rstest]
    fn test_all_topics_includes_confirmed_and_pending_subscribe() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.mark_subscribe("tickers.ETHUSDT");

        let topics = state.all_topics();
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&"tickers.BTCUSDT".to_string()));
        assert!(topics.contains(&"tickers.ETHUSDT".to_string()));
    }

    #[rstest]
    fn test_all_topics_excludes_pending_unsubscribe() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.mark_unsubscribe("tickers.BTCUSDT");

        let topics = state.all_topics();
        assert!(topics.is_empty());
    }

    #[rstest]
    fn test_reference_counting_single_topic() {
        let state = SubscriptionState::new('.');

        // First subscription
        assert!(state.add_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 1);

        // Second subscription
        assert!(!state.add_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 2);

        // First removal
        assert!(!state.remove_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 1);

        // Second removal
        assert!(state.remove_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 0);
    }

    #[rstest]
    fn test_reference_counting_multiple_topics() {
        let state = SubscriptionState::new('.');

        assert!(state.add_reference("tickers.BTCUSDT"));
        assert!(state.add_reference("tickers.ETHUSDT"));

        assert!(!state.add_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 2);
        assert_eq!(state.get_reference_count("tickers.ETHUSDT"), 1);

        assert!(!state.remove_reference("tickers.BTCUSDT"));
        assert!(state.remove_reference("tickers.ETHUSDT"));
    }

    #[rstest]
    fn test_topic_without_symbol() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("orderbook");
        state.confirm_subscribe("orderbook");

        assert_eq!(state.len(), 1);
        assert_eq!(state.all_topics(), vec!["orderbook"]);
    }

    #[rstest]
    fn test_different_delimiters() {
        let state_dot = SubscriptionState::new('.');
        state_dot.mark_subscribe("tickers.BTCUSDT");
        assert_eq!(
            state_dot.pending_subscribe_topics(),
            vec!["tickers.BTCUSDT"]
        );

        let state_colon = SubscriptionState::new(':');
        state_colon.mark_subscribe("orderBookL2:XBTUSD");
        assert_eq!(
            state_colon.pending_subscribe_topics(),
            vec!["orderBookL2:XBTUSD"]
        );
    }

    #[rstest]
    fn test_clear() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.add_reference("tickers.BTCUSDT");

        state.clear();

        assert!(state.is_empty());
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 0);
    }

    #[rstest]
    fn test_multiple_symbols_same_channel() {
        let state = SubscriptionState::new('.');
        state.mark_subscribe("tickers.BTCUSDT");
        state.mark_subscribe("tickers.ETHUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.ETHUSDT");

        assert_eq!(state.len(), 2);
        let topics = state.all_topics();
        assert!(topics.contains(&"tickers.BTCUSDT".to_string()));
        assert!(topics.contains(&"tickers.ETHUSDT".to_string()));
    }

    #[rstest]
    fn test_mixed_channel_and_symbol_subscriptions() {
        let state = SubscriptionState::new('.');

        // Subscribe to channel-level first
        state.mark_subscribe("tickers");
        state.confirm_subscribe("tickers");
        assert_eq!(state.len(), 1);
        assert_eq!(state.all_topics(), vec!["tickers"]);

        // Add symbol-level subscription to same channel
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 2);

        // Both should be present
        let topics = state.all_topics();
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&"tickers".to_string()));
        assert!(topics.contains(&"tickers.BTCUSDT".to_string()));

        // Add another symbol
        state.mark_subscribe("tickers.ETHUSDT");
        state.confirm_subscribe("tickers.ETHUSDT");
        assert_eq!(state.len(), 3);

        let topics = state.all_topics();
        assert_eq!(topics.len(), 3);
        assert!(topics.contains(&"tickers".to_string()));
        assert!(topics.contains(&"tickers.BTCUSDT".to_string()));
        assert!(topics.contains(&"tickers.ETHUSDT".to_string()));

        // Unsubscribe from channel-level only
        state.mark_unsubscribe("tickers");
        state.confirm_unsubscribe("tickers");
        assert_eq!(state.len(), 2);

        let topics = state.all_topics();
        assert_eq!(topics.len(), 2);
        assert!(!topics.contains(&"tickers".to_string()));
        assert!(topics.contains(&"tickers.BTCUSDT".to_string()));
        assert!(topics.contains(&"tickers.ETHUSDT".to_string()));
    }

    #[rstest]
    fn test_symbol_subscription_before_channel() {
        let state = SubscriptionState::new('.');

        // Subscribe to symbol first
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);

        // Then add channel-level
        state.mark_subscribe("tickers");
        state.confirm_subscribe("tickers");
        assert_eq!(state.len(), 2);

        // Both should be present after reconnect
        let topics = state.all_topics();
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&"tickers".to_string()));
        assert!(topics.contains(&"tickers.BTCUSDT".to_string()));
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_subscribe_same_topic() {
        let state = Arc::new(SubscriptionState::new('.'));
        let mut handles = vec![];

        // Spawn 10 tasks all subscribing to the same topic
        for _ in 0..10 {
            let state_clone = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                state_clone.add_reference("tickers.BTCUSDT");
                state_clone.mark_subscribe("tickers.BTCUSDT");
                state_clone.confirm_subscribe("tickers.BTCUSDT");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Reference count should be exactly 10
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 10);
        assert_eq!(state.len(), 1);
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_subscribe_unsubscribe() {
        let state = Arc::new(SubscriptionState::new('.'));
        let mut handles = vec![];

        // Spawn 20 tasks, each adding 2 references to their own unique topic
        // This ensures deterministic behavior - we know exactly what the final state should be
        for i in 0..20 {
            let state_clone = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                let topic = format!("tickers.SYMBOL{i}");
                // Add 2 references
                state_clone.add_reference(&topic);
                state_clone.add_reference(&topic);
                state_clone.mark_subscribe(&topic);
                state_clone.confirm_subscribe(&topic);

                // Remove 1 reference (should still have 1 remaining)
                state_clone.remove_reference(&topic);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Each of the 20 topics should still have 1 reference
        for i in 0..20 {
            let topic = format!("tickers.SYMBOL{i}");
            assert_eq!(state.get_reference_count(&topic), 1);
        }

        // Should have exactly 20 confirmed subscriptions
        assert_eq!(state.len(), 20);
        assert!(!state.is_empty());
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_reference_counting_same_topic() {
        let state = Arc::new(SubscriptionState::new('.'));
        let topic = "tickers.BTCUSDT";
        let mut handles = vec![];

        // Spawn 10 tasks all adding 10 references to the same topic
        for _ in 0..10 {
            let state_clone = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                for _ in 0..10 {
                    state_clone.add_reference(topic);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Should have exactly 100 references (10 tasks * 10 refs each)
        assert_eq!(state.get_reference_count(topic), 100);

        // Now remove 50 references sequentially
        for _ in 0..50 {
            state.remove_reference(topic);
        }

        // Should have exactly 50 references remaining
        assert_eq!(state.get_reference_count(topic), 50);
    }

    #[rstest]
    fn test_reconnection_scenario() {
        let state = SubscriptionState::new('.');

        // Initial subscriptions
        state.add_reference("tickers.BTCUSDT");
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");

        state.add_reference("tickers.ETHUSDT");
        state.mark_subscribe("tickers.ETHUSDT");
        state.confirm_subscribe("tickers.ETHUSDT");

        state.add_reference("orderbook");
        state.mark_subscribe("orderbook");
        state.confirm_subscribe("orderbook");

        assert_eq!(state.len(), 3);

        // Simulate disconnect - topics should be available for resubscription
        let topics_to_resubscribe = state.all_topics();
        assert_eq!(topics_to_resubscribe.len(), 3);
        assert!(topics_to_resubscribe.contains(&"tickers.BTCUSDT".to_string()));
        assert!(topics_to_resubscribe.contains(&"tickers.ETHUSDT".to_string()));
        assert!(topics_to_resubscribe.contains(&"orderbook".to_string()));

        // On reconnect, mark all as pending again
        for topic in &topics_to_resubscribe {
            state.mark_subscribe(topic);
        }

        // Simulate server confirmations
        for topic in &topics_to_resubscribe {
            state.confirm_subscribe(topic);
        }

        // Should still have all 3 subscriptions
        assert_eq!(state.len(), 3);
        assert_eq!(state.all_topics().len(), 3);
    }

    #[rstest]
    fn test_state_machine_invalid_transitions() {
        let state = SubscriptionState::new('.');

        // Confirm subscribe without marking first - should not crash
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1); // Gets added to confirmed

        // Confirm unsubscribe without marking first - should not crash
        state.confirm_unsubscribe("tickers.ETHUSDT");
        assert_eq!(state.len(), 1); // Nothing changes

        // Double confirm subscribe
        state.mark_subscribe("orderbook");
        state.confirm_subscribe("orderbook");
        state.confirm_subscribe("orderbook"); // Second confirm is idempotent
        assert_eq!(state.len(), 2);

        // Unsubscribe something that was never subscribed
        state.mark_unsubscribe("nonexistent");
        state.confirm_unsubscribe("nonexistent");
        assert_eq!(state.len(), 2); // Still 2
    }

    #[rstest]
    fn test_mark_failure_moves_to_pending() {
        let state = SubscriptionState::new('.');

        // Subscribe and confirm
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);
        assert!(state.pending_subscribe_topics().is_empty());

        // Mark as failed
        state.mark_failure("tickers.BTCUSDT");

        // Should be removed from confirmed and back in pending
        assert_eq!(state.len(), 0);
        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]);

        // all_topics should still include it for reconnection
        assert_eq!(state.all_topics(), vec!["tickers.BTCUSDT"]);
    }

    #[rstest]
    fn test_pending_subscribe_excludes_pending_unsubscribe() {
        let state = SubscriptionState::new('.');

        // Subscribe and confirm
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");

        // Mark for unsubscribe
        state.mark_unsubscribe("tickers.BTCUSDT");

        // Should be in pending_unsubscribe but NOT in all_topics
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);
        assert!(state.all_topics().is_empty());
        assert_eq!(state.len(), 0);
    }

    #[rstest]
    fn test_remove_reference_nonexistent_topic() {
        let state = SubscriptionState::new('.');

        // Removing reference to topic that was never added
        let should_unsubscribe = state.remove_reference("nonexistent");

        // Should return false and not crash
        assert!(!should_unsubscribe);
        assert_eq!(state.get_reference_count("nonexistent"), 0);
    }

    #[rstest]
    fn test_edge_case_empty_channel_name() {
        let state = SubscriptionState::new('.');

        // Edge case: empty string as topic
        state.mark_subscribe("");
        state.confirm_subscribe("");

        assert_eq!(state.len(), 1);
        assert_eq!(state.all_topics(), vec![""]);
    }

    #[rstest]
    fn test_state_transitions_table() {
        // Table-driven test for all state transitions
        let test_cases = vec![
            (
                "mark_subscribe → confirm_subscribe",
                vec!["mark_subscribe", "confirm_subscribe"],
                1, // expected confirmed count
                0, // expected pending count
            ),
            (
                "mark_subscribe → mark_unsubscribe → confirm_unsubscribe",
                vec!["mark_subscribe", "mark_unsubscribe", "confirm_unsubscribe"],
                0,
                0, // Removed from pending_subscribe when marked for unsubscribe
            ),
            (
                "mark_subscribe → confirm_subscribe → mark_unsubscribe",
                vec!["mark_subscribe", "confirm_subscribe", "mark_unsubscribe"],
                0, // Removed from confirmed
                0,
            ),
            (
                "mark_subscribe → confirm_subscribe → mark_failure",
                vec!["mark_subscribe", "confirm_subscribe", "mark_failure"],
                0, // Moved to pending
                1, // Now in pending_subscribe
            ),
            (
                "confirm_subscribe (without mark)",
                vec!["confirm_subscribe"],
                1, // Directly added to confirmed
                0,
            ),
        ];

        for (name, operations, expected_confirmed, expected_pending) in test_cases {
            let state = SubscriptionState::new('.');
            let topic = "test.TOPIC";

            for op in operations {
                match op {
                    "mark_subscribe" => state.mark_subscribe(topic),
                    "confirm_subscribe" => state.confirm_subscribe(topic),
                    "mark_unsubscribe" => state.mark_unsubscribe(topic),
                    "confirm_unsubscribe" => state.confirm_unsubscribe(topic),
                    "mark_failure" => state.mark_failure(topic),
                    _ => panic!("Unknown operation: {op}"),
                }
            }

            assert_eq!(
                state.len(),
                expected_confirmed,
                "Failed for case: {name} (confirmed count mismatch)"
            );
            assert_eq!(
                state.pending_subscribe_topics().len(),
                expected_pending,
                "Failed for case: {name} (pending count mismatch)"
            );
        }
    }

    #[rstest]
    fn test_duplicate_operations() {
        let state = SubscriptionState::new('.');

        // Multiple mark_subscribe on same topic
        state.mark_subscribe("tickers.BTCUSDT");
        state.mark_subscribe("tickers.BTCUSDT");
        state.mark_subscribe("tickers.BTCUSDT");

        let pending = state.pending_subscribe_topics();
        assert_eq!(pending.len(), 1); // Should not duplicate

        // Multiple confirm_subscribe
        state.confirm_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);

        // Multiple mark_unsubscribe
        state.mark_unsubscribe("tickers.BTCUSDT");
        state.mark_unsubscribe("tickers.BTCUSDT");
        let pending_unsub = state.pending_unsubscribe_topics();
        assert_eq!(pending_unsub.len(), 1); // Should not duplicate
    }

    #[rstest]
    fn test_reference_count_edge_cases() {
        let state = SubscriptionState::new('.');
        let topic = "tickers.BTCUSDT";

        // Add and remove to test boundary
        assert!(state.add_reference(topic)); // First ref
        assert!(!state.add_reference(topic)); // Second ref
        assert!(!state.remove_reference(topic)); // Down to 1
        assert!(state.remove_reference(topic)); // Down to 0 - should unsubscribe

        // Remove when count is already 0 - should be safe
        assert!(!state.remove_reference(topic));
        assert!(!state.remove_reference(topic));
        assert_eq!(state.get_reference_count(topic), 0);

        // Add again after going to 0
        assert!(state.add_reference(topic)); // Should be first ref again
        assert_eq!(state.get_reference_count(topic), 1);
    }

    #[rstest]
    fn test_special_characters_in_topics() {
        let state = SubscriptionState::new('.');

        // Topics with special characters
        let special_topics = vec![
            "channel.symbol-with-dash",
            "channel.SYMBOL_WITH_UNDERSCORE",
            "channel.symbol123",
            "channel.symbol@special",
        ];

        for topic in &special_topics {
            state.mark_subscribe(topic);
            state.confirm_subscribe(topic);
        }

        assert_eq!(state.len(), special_topics.len());

        let all_topics = state.all_topics();
        for topic in &special_topics {
            assert!(
                all_topics.contains(&(*topic).to_string()),
                "Missing topic: {topic}"
            );
        }
    }

    #[rstest]
    fn test_clear_resets_all_state() {
        let state = SubscriptionState::new('.');

        // Add multiple subscriptions and references
        for i in 0..10 {
            let topic = format!("channel{i}.SYMBOL");
            state.add_reference(&topic);
            state.add_reference(&topic); // Add twice
            state.mark_subscribe(&topic);
            state.confirm_subscribe(&topic);
        }

        assert_eq!(state.len(), 10);
        assert!(!state.is_empty());

        // Clear everything
        state.clear();

        // Verify complete reset
        assert_eq!(state.len(), 0);
        assert!(state.is_empty());
        assert!(state.all_topics().is_empty());
        assert!(state.pending_subscribe_topics().is_empty());
        assert!(state.pending_unsubscribe_topics().is_empty());

        // Verify reference counts are cleared
        for i in 0..10 {
            let topic = format!("channel{i}.SYMBOL");
            assert_eq!(state.get_reference_count(&topic), 0);
        }
    }

    #[rstest]
    fn test_different_delimiter_does_not_affect_storage() {
        // Verify delimiter is only used for parsing, not storage
        let state_dot = SubscriptionState::new('.');
        let state_colon = SubscriptionState::new(':');

        // Add same logical subscription with different delimiters
        state_dot.mark_subscribe("channel.SYMBOL");
        state_colon.mark_subscribe("channel:SYMBOL");

        // Both should work correctly
        assert_eq!(state_dot.pending_subscribe_topics(), vec!["channel.SYMBOL"]);
        assert_eq!(
            state_colon.pending_subscribe_topics(),
            vec!["channel:SYMBOL"]
        );
    }

    #[rstest]
    fn test_unsubscribe_before_subscribe_confirmed() {
        let state = SubscriptionState::new('.');

        // User subscribes
        state.mark_subscribe("tickers.BTCUSDT");
        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]);

        // User immediately changes mind before server confirms
        state.mark_unsubscribe("tickers.BTCUSDT");

        // Should be removed from pending_subscribe and added to pending_unsubscribe
        assert!(state.pending_subscribe_topics().is_empty());
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);

        // Confirm the unsubscribe
        state.confirm_unsubscribe("tickers.BTCUSDT");

        // Should be completely gone
        assert!(state.is_empty());
        assert!(state.all_topics().is_empty());
        assert_eq!(state.len(), 0);
    }

    #[rstest]
    fn test_late_subscribe_confirmation_after_unsubscribe() {
        let state = SubscriptionState::new('.');

        // User subscribes
        state.mark_subscribe("tickers.BTCUSDT");

        // User immediately unsubscribes
        state.mark_unsubscribe("tickers.BTCUSDT");

        // Late subscribe confirmation arrives from server
        state.confirm_subscribe("tickers.BTCUSDT");

        // Should NOT be added to confirmed (unsubscribe takes precedence)
        assert_eq!(state.len(), 0);
        assert!(state.pending_subscribe_topics().is_empty());

        // Confirm the unsubscribe
        state.confirm_unsubscribe("tickers.BTCUSDT");

        // Should still be empty
        assert!(state.is_empty());
        assert!(state.all_topics().is_empty());
    }

    #[rstest]
    fn test_unsubscribe_clears_all_states() {
        let state = SubscriptionState::new('.');

        // Subscribe and confirm
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);

        // Unsubscribe
        state.mark_unsubscribe("tickers.BTCUSDT");

        // Should be removed from confirmed
        assert_eq!(state.len(), 0);
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);

        // Late subscribe confirmation somehow arrives (race condition)
        state.confirm_subscribe("tickers.BTCUSDT");

        // confirm_unsubscribe should clean everything
        state.confirm_unsubscribe("tickers.BTCUSDT");

        // Completely empty
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);
        assert!(state.pending_subscribe_topics().is_empty());
        assert!(state.pending_unsubscribe_topics().is_empty());
        assert!(state.all_topics().is_empty());
    }

    #[rstest]
    fn test_mark_failure_respects_pending_unsubscribe() {
        let state = SubscriptionState::new('.');

        // Subscribe and confirm
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);

        // User unsubscribes
        state.mark_unsubscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 0);
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);

        // Meanwhile, a network error triggers mark_failure
        state.mark_failure("tickers.BTCUSDT");

        // Should NOT be added to pending_subscribe (user wanted to unsubscribe)
        assert!(state.pending_subscribe_topics().is_empty());
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);

        // all_topics should NOT include it
        assert!(state.all_topics().is_empty());

        // Confirm unsubscribe
        state.confirm_unsubscribe("tickers.BTCUSDT");
        assert!(state.is_empty());
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_concurrent_stress_mixed_operations() {
        let state = Arc::new(SubscriptionState::new('.'));
        let mut handles = vec![];

        // Spawn 50 tasks doing random interleaved operations
        for i in 0..50 {
            let state_clone = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                let topic1 = format!("channel.SYMBOL{i}");
                let topic2 = format!("channel.SYMBOL{}", i + 100);

                // Add references
                state_clone.add_reference(&topic1);
                state_clone.add_reference(&topic2);

                // Mark and confirm subscriptions
                state_clone.mark_subscribe(&topic1);
                state_clone.confirm_subscribe(&topic1);
                state_clone.mark_subscribe(&topic2);

                // Interleave some unsubscribes
                if i % 3 == 0 {
                    state_clone.mark_unsubscribe(&topic1);
                    state_clone.confirm_unsubscribe(&topic1);
                }

                // More reference operations
                state_clone.add_reference(&topic2);
                state_clone.remove_reference(&topic2);

                // Confirm topic2
                state_clone.confirm_subscribe(&topic2);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify state is consistent (no panics, all maps accessible)
        let all = state.all_topics();
        let confirmed_count = state.len();

        // We have 50 topic2s (always confirmed) + topic1s (50 - number unsubscribed)
        // About 17 topic1s get unsubscribed (i % 3 == 0), leaving ~33 topic1s + 50 topic2s = ~83
        assert!(confirmed_count > 50); // At least all topic2s
        assert!(confirmed_count <= 100); // At most all topic1s + topic2s
        assert_eq!(
            all.len(),
            confirmed_count + state.pending_subscribe_topics().len()
        );
    }

    #[rstest]
    fn test_expanded_state_transitions() {
        let test_cases = vec![
            (
                "subscribe → failure → confirm_subscribe (late)",
                vec!["mark_subscribe", "mark_failure", "confirm_subscribe"],
                1, // Moved back to pending, then late confirm adds to confirmed
                0,
            ),
            (
                "subscribe → confirm → failure → confirm_subscribe again",
                vec![
                    "mark_subscribe",
                    "confirm_subscribe",
                    "mark_failure",
                    "confirm_subscribe",
                ],
                1, // Failed and back to confirmed
                0,
            ),
            (
                "subscribe → unsubscribe → failure (should ignore)",
                vec!["mark_subscribe", "mark_unsubscribe", "mark_failure"],
                0, // Failure ignored due to pending unsubscribe
                0, // Cleared from pending_subscribe by mark_unsubscribe
            ),
            (
                "confirm (direct) → unsubscribe → confirm_unsubscribe",
                vec![
                    "confirm_subscribe",
                    "mark_unsubscribe",
                    "confirm_unsubscribe",
                ],
                0,
                0,
            ),
            (
                "subscribe → confirm → unsubscribe → late confirm_subscribe",
                vec![
                    "mark_subscribe",
                    "confirm_subscribe",
                    "mark_unsubscribe",
                    "confirm_subscribe",
                ],
                0, // Late confirm ignored due to pending_unsubscribe
                0,
            ),
            (
                "multiple failures in a row",
                vec![
                    "mark_subscribe",
                    "confirm_subscribe",
                    "mark_failure",
                    "mark_failure",
                    "mark_failure",
                ],
                0, // Multiple failures are idempotent
                1, // Still just one pending
            ),
            (
                "subscribe → failure before confirm",
                vec!["mark_subscribe", "mark_failure"],
                0,
                1, // Stays in pending (was already there)
            ),
            (
                "empty state → unsubscribe (no-op)",
                vec!["mark_unsubscribe", "confirm_unsubscribe"],
                0,
                0, // Should not crash
            ),
        ];

        for (name, operations, expected_confirmed, expected_pending) in test_cases {
            let state = SubscriptionState::new('.');
            let topic = "test.TOPIC";

            for op in operations {
                match op {
                    "mark_subscribe" => state.mark_subscribe(topic),
                    "confirm_subscribe" => state.confirm_subscribe(topic),
                    "mark_unsubscribe" => state.mark_unsubscribe(topic),
                    "confirm_unsubscribe" => state.confirm_unsubscribe(topic),
                    "mark_failure" => state.mark_failure(topic),
                    _ => panic!("Unknown operation: {op}"),
                }
            }

            assert_eq!(
                state.len(),
                expected_confirmed,
                "Failed for case: {name} (confirmed count mismatch)"
            );
            assert_eq!(
                state.pending_subscribe_topics().len(),
                expected_pending,
                "Failed for case: {name} (pending count mismatch)"
            );
        }
    }

    #[rstest]
    fn test_edge_case_malformed_topics() {
        let state = SubscriptionState::new('.');

        // Topics with multiple delimiters (splits on first delimiter)
        state.mark_subscribe("channel.symbol.extra");
        state.confirm_subscribe("channel.symbol.extra");
        let topics = state.all_topics();
        assert!(topics.contains(&"channel.symbol.extra".to_string()));

        // Topic with leading delimiter (empty channel, symbol is "channel")
        state.mark_subscribe(".channel");
        state.confirm_subscribe(".channel");
        assert_eq!(state.len(), 2);

        // Topic with trailing delimiter - treated as channel-level (empty symbol = marker)
        // "channel." splits to ("channel", Some("")), and empty string is the channel marker
        state.mark_subscribe("channel.");
        state.confirm_subscribe("channel.");
        assert_eq!(state.len(), 3);

        // Topic without delimiter - explicitly channel-level
        state.mark_subscribe("tickers");
        state.confirm_subscribe("tickers");
        assert_eq!(state.len(), 4);

        // Verify all are retrievable (note: "channel." becomes "channel")
        let all = state.all_topics();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&"channel.symbol.extra".to_string()));
        assert!(all.contains(&".channel".to_string()));
        assert!(all.contains(&"channel".to_string())); // "channel." treated as channel-level
        assert!(all.contains(&"tickers".to_string()));
    }

    #[rstest]
    fn test_reference_count_underflow_safety() {
        let state = SubscriptionState::new('.');

        // Remove without ever adding
        assert!(!state.remove_reference("never.added"));
        assert_eq!(state.get_reference_count("never.added"), 0);

        // Add one, remove multiple times
        state.add_reference("once.added");
        assert_eq!(state.get_reference_count("once.added"), 1);

        assert!(state.remove_reference("once.added")); // Should return true (last ref)
        assert_eq!(state.get_reference_count("once.added"), 0);

        assert!(!state.remove_reference("once.added")); // Should not crash, returns false
        assert!(!state.remove_reference("once.added")); // Multiple times
        assert_eq!(state.get_reference_count("once.added"), 0);

        // Verify we can add again after underflow attempts
        assert!(state.add_reference("once.added"));
        assert_eq!(state.get_reference_count("once.added"), 1);
    }

    #[rstest]
    fn test_reconnection_with_partial_state() {
        let state = SubscriptionState::new('.');

        // Setup: Some confirmed, some pending subscribe, some pending unsubscribe
        // Confirmed
        state.mark_subscribe("confirmed.BTCUSDT");
        state.confirm_subscribe("confirmed.BTCUSDT");

        // Pending subscribe (not yet confirmed)
        state.mark_subscribe("pending.ETHUSDT");

        // Pending unsubscribe (user cancelled)
        state.mark_subscribe("cancelled.XRPUSDT");
        state.confirm_subscribe("cancelled.XRPUSDT");
        state.mark_unsubscribe("cancelled.XRPUSDT");

        // Verify state before reconnect
        assert_eq!(state.len(), 1); // Only confirmed.BTCUSDT
        let all = state.all_topics();
        assert_eq!(all.len(), 2); // confirmed + pending_subscribe (not pending_unsubscribe)
        assert!(all.contains(&"confirmed.BTCUSDT".to_string()));
        assert!(all.contains(&"pending.ETHUSDT".to_string()));
        assert!(!all.contains(&"cancelled.XRPUSDT".to_string())); // Should NOT be included

        // Simulate disconnect and reconnect
        let topics_to_resubscribe = state.all_topics();

        // Clear confirmed on disconnect (simulate connection drop)
        state.confirmed().clear();

        // Mark all for resubscription
        for topic in &topics_to_resubscribe {
            state.mark_subscribe(topic);
        }

        // Server confirms both
        for topic in &topics_to_resubscribe {
            state.confirm_subscribe(topic);
        }

        // Verify final state
        assert_eq!(state.len(), 2); // Both confirmed
        let final_topics = state.all_topics();
        assert_eq!(final_topics.len(), 2);
        assert!(final_topics.contains(&"confirmed.BTCUSDT".to_string()));
        assert!(final_topics.contains(&"pending.ETHUSDT".to_string()));
        assert!(!final_topics.contains(&"cancelled.XRPUSDT".to_string()));
    }
}

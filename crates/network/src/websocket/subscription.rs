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
//! production.
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
//! - Dot delimiter: `tickers.BTCUSDT`
//! - Colon delimiter: `trades:BTC-USDT`
//!
//! Channels without symbols are also supported (e.g., `execution` for all instruments).

use std::{
    num::NonZeroUsize,
    sync::{Arc, LazyLock},
};

use ahash::AHashSet;
use dashmap::DashMap;
use ustr::Ustr;

/// Marker for channel-level subscriptions (no specific symbol).
///
/// An empty string in the symbol set indicates a channel-level subscription
/// that applies to all symbols for that channel.
pub(crate) static CHANNEL_LEVEL_MARKER: LazyLock<Ustr> = LazyLock::new(|| Ustr::from(""));

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
    /// Idempotent: if topic is already confirmed, this is a no-op.
    /// If topic is pending unsubscription, removes it.
    pub fn mark_subscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);

        // If already confirmed, don't re-add to pending (idempotent)
        if is_tracked(&self.confirmed, channel, symbol) {
            return;
        }

        // Remove from pending_unsubscribe if present
        untrack_topic(&self.pending_unsubscribe, channel, symbol);

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

    /// Confirms an unsubscription by removing it from pending and confirmed state.
    ///
    /// This should be called when the server acknowledges an unsubscribe request.
    /// Removes the topic from pending_unsubscribe and confirmed.
    /// Does NOT clear pending_subscribe to support immediate re-subscribe patterns
    /// (e.g., user calls subscribe() before unsubscribe ack arrives).
    ///
    /// **Stale ACK handling**: Ignores unsubscribe ACKs if the topic is no longer
    /// in pending_unsubscribe (meaning user has already re-subscribed). This prevents
    /// stale ACKs from removing topics that were re-confirmed after the re-subscribe.
    pub fn confirm_unsubscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic, self.delimiter);

        // Only process if topic is actually pending unsubscription
        // This ignores stale unsubscribe ACKs after user has re-subscribed
        if !is_tracked(&self.pending_unsubscribe, channel, symbol) {
            return; // Stale ACK, ignore
        }

        untrack_topic(&self.pending_unsubscribe, channel, symbol);
        untrack_topic(&self.confirmed, channel, symbol);
        // Don't clear pending_subscribe - it's a valid re-subscribe request
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
        let marker = *CHANNEL_LEVEL_MARKER;

        for entry in map {
            let channel = entry.key();
            let symbols = entry.value();

            // Check for channel-level subscription marker
            if symbols.contains(&marker) {
                topics.push(channel.to_string());
            }

            // Add symbol-level subscriptions (skip marker)
            for symbol in symbols {
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

        // Use entry API to atomically decrement and remove if zero
        // This prevents race where another thread adds a reference between the check and remove
        if let dashmap::mapref::entry::Entry::Occupied(mut entry) =
            self.reference_counts.entry(topic_ustr)
        {
            let current = entry.get().get();

            if current == 1 {
                entry.remove();
                return true;
            }

            *entry.get_mut() = NonZeroUsize::new(current - 1)
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
        entry.insert(*CHANNEL_LEVEL_MARKER);
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
        *CHANNEL_LEVEL_MARKER
    };

    // Use entry API to atomically remove symbol and check if empty
    // This prevents race conditions where another thread adds a symbol between operations
    if let dashmap::mapref::entry::Entry::Occupied(mut entry) = map.entry(channel_ustr) {
        entry.get_mut().remove(&symbol_to_remove);
        if entry.get().is_empty() {
            entry.remove();
        }
    }
}

/// Checks if a topic exists in the given map.
fn is_tracked(map: &DashMap<Ustr, AHashSet<Ustr>>, channel: &str, symbol: Option<&str>) -> bool {
    let channel_ustr = Ustr::from(channel);
    let symbol_to_check = if let Some(symbol) = symbol {
        Ustr::from(symbol)
    } else {
        *CHANNEL_LEVEL_MARKER
    };

    if let Some(entry) = map.get(&channel_ustr) {
        entry.contains(&symbol_to_check)
    } else {
        false
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
    fn test_resubscribe_before_unsubscribe_ack() {
        // Regression test for race condition:
        // User unsubscribes, then immediately resubscribes before the unsubscribe ACK arrives.
        // The unsubscribe ACK should NOT clear the pending_subscribe entry.
        let state = SubscriptionState::new('.');

        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);

        state.mark_unsubscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 0);
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);

        // User immediately resubscribes (before unsubscribe ACK)
        state.mark_subscribe("tickers.BTCUSDT");
        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]);

        // Stale unsubscribe ACK arrives - should be ignored (pending_unsubscribe already cleared)
        state.confirm_unsubscribe("tickers.BTCUSDT");
        assert!(state.pending_unsubscribe_topics().is_empty());
        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]); // Must still be pending

        // Subscribe ACK confirms successfully
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);
        assert!(state.pending_subscribe_topics().is_empty());

        // Topic available for reconnect
        let all = state.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&"tickers.BTCUSDT".to_string()));
    }

    #[rstest]
    fn test_stale_unsubscribe_ack_after_resubscribe_confirmed() {
        // Regression test for P1 bug: Stale unsubscribe ACK removing confirmed topic.
        // Scenario: User unsubscribes, immediately resubscribes, subscribe ACK arrives
        // FIRST (out of order), then stale unsubscribe ACK arrives.
        // The stale ACK must NOT remove the topic from confirmed state.
        let state = SubscriptionState::new('.');

        // Initial subscription
        state.mark_subscribe("tickers.BTCUSDT");
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1);

        // User unsubscribes
        state.mark_unsubscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 0);
        assert_eq!(state.pending_unsubscribe_topics(), vec!["tickers.BTCUSDT"]);

        // User immediately resubscribes (before unsubscribe ACK)
        state.mark_subscribe("tickers.BTCUSDT");
        assert!(state.pending_unsubscribe_topics().is_empty()); // Cleared by mark_subscribe
        assert_eq!(state.pending_subscribe_topics(), vec!["tickers.BTCUSDT"]);

        // Subscribe ACK arrives FIRST (out of order!)
        state.confirm_subscribe("tickers.BTCUSDT");
        assert_eq!(state.len(), 1); // Back in confirmed
        assert!(state.pending_subscribe_topics().is_empty());

        // NOW the stale unsubscribe ACK arrives
        // This must be ignored because topic is no longer in pending_unsubscribe
        state.confirm_unsubscribe("tickers.BTCUSDT");

        // Topic should STILL be confirmed (not removed by stale ACK)
        assert_eq!(state.len(), 1); // Must remain confirmed
        assert!(state.pending_unsubscribe_topics().is_empty());
        assert!(state.pending_subscribe_topics().is_empty());

        // Topic should be in all_topics (for reconnect)
        let all = state.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&"tickers.BTCUSDT".to_string()));
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

        assert!(state.add_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 1);

        assert!(!state.add_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 2);

        assert!(!state.remove_reference("tickers.BTCUSDT"));
        assert_eq!(state.get_reference_count("tickers.BTCUSDT"), 1);

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

    /// Verifies all invariants of the subscription state.
    ///
    /// # Invariants
    ///
    /// 1. **Mutual exclusivity**: A topic cannot exist in multiple states simultaneously
    ///    (one of: confirmed, pending_subscribe, pending_unsubscribe, or none).
    /// 2. **all_topics consistency**: `all_topics()` must equal `confirmed ∪ pending_subscribe`
    /// 3. **len consistency**: `len()` must equal total count of symbols in confirmed map
    /// 4. **is_empty consistency**: `is_empty()` true iff all maps are empty
    /// 5. **Reference count non-negative**: All reference counts >= 0
    fn check_invariants(state: &SubscriptionState, label: &str) {
        // Collect all topics from each state
        let confirmed_topics: AHashSet<String> = state
            .topics_from_map(&state.confirmed)
            .into_iter()
            .collect();
        let pending_sub_topics: AHashSet<String> =
            state.pending_subscribe_topics().into_iter().collect();
        let pending_unsub_topics: AHashSet<String> =
            state.pending_unsubscribe_topics().into_iter().collect();

        // INVARIANT 1: Mutual exclusivity - no topic in multiple states
        let confirmed_and_pending_sub: Vec<_> =
            confirmed_topics.intersection(&pending_sub_topics).collect();
        assert!(
            confirmed_and_pending_sub.is_empty(),
            "{label}: Topic in both confirmed and pending_subscribe: {confirmed_and_pending_sub:?}"
        );

        let confirmed_and_pending_unsub: Vec<_> = confirmed_topics
            .intersection(&pending_unsub_topics)
            .collect();
        assert!(
            confirmed_and_pending_unsub.is_empty(),
            "{label}: Topic in both confirmed and pending_unsubscribe: {confirmed_and_pending_unsub:?}"
        );

        let pending_sub_and_unsub: Vec<_> = pending_sub_topics
            .intersection(&pending_unsub_topics)
            .collect();
        assert!(
            pending_sub_and_unsub.is_empty(),
            "{label}: Topic in both pending_subscribe and pending_unsubscribe: {pending_sub_and_unsub:?}"
        );

        // INVARIANT 2: all_topics() == confirmed ∪ pending_subscribe
        let all_topics: AHashSet<String> = state.all_topics().into_iter().collect();
        let expected_all: AHashSet<String> = confirmed_topics
            .union(&pending_sub_topics)
            .cloned()
            .collect();
        assert_eq!(
            all_topics, expected_all,
            "{label}: all_topics() doesn't match confirmed ∪ pending_subscribe"
        );

        // Ensure pending_unsubscribe is NOT in all_topics
        for topic in &pending_unsub_topics {
            assert!(
                !all_topics.contains(topic),
                "{label}: pending_unsubscribe topic {topic} incorrectly in all_topics()"
            );
        }

        // INVARIANT 3: len() == sum of confirmed symbol counts
        let expected_len: usize = state
            .confirmed
            .iter()
            .map(|entry| entry.value().len())
            .sum();
        assert_eq!(
            state.len(),
            expected_len,
            "{label}: len() mismatch. Expected {expected_len}, was {}",
            state.len()
        );

        // INVARIANT 4: is_empty() consistency
        let should_be_empty = state.confirmed.is_empty()
            && pending_sub_topics.is_empty()
            && pending_unsub_topics.is_empty();
        assert_eq!(
            state.is_empty(),
            should_be_empty,
            "{label}: is_empty() inconsistent. Maps empty: {should_be_empty}, is_empty(): {}",
            state.is_empty()
        );

        // INVARIANT 5: Reference counts non-negative (NonZeroUsize enforces > 0, absence = 0)
        for entry in state.reference_counts.iter() {
            let count = entry.value().get();
            assert!(
                count > 0,
                "{label}: Reference count should be NonZeroUsize (> 0), was {count} for {:?}",
                entry.key()
            );
        }
    }

    /// Checks that a topic exists in exactly one of the three states or none.
    fn check_topic_exclusivity(state: &SubscriptionState, topic: &str, label: &str) {
        let (channel, symbol) = split_topic(topic, state.delimiter);

        let in_confirmed = is_tracked(&state.confirmed, channel, symbol);
        let in_pending_sub = is_tracked(&state.pending_subscribe, channel, symbol);
        let in_pending_unsub = is_tracked(&state.pending_unsubscribe, channel, symbol);

        let count = [in_confirmed, in_pending_sub, in_pending_unsub]
            .iter()
            .filter(|&&x| x)
            .count();

        assert!(
            count <= 1,
            "{label}: Topic {topic} in {count} states (should be 0 or 1). \
             confirmed: {in_confirmed}, pending_sub: {in_pending_sub}, pending_unsub: {in_pending_unsub}"
        );
    }

    #[cfg(test)]
    mod property_tests {
        use proptest::prelude::*;

        use super::*;

        #[derive(Debug, Clone)]
        enum Operation {
            MarkSubscribe(String),
            ConfirmSubscribe(String),
            MarkUnsubscribe(String),
            ConfirmUnsubscribe(String),
            MarkFailure(String),
            AddReference(String),
            RemoveReference(String),
            Clear,
        }

        // Strategy for generating valid topics
        fn topic_strategy() -> impl Strategy<Value = String> {
            prop_oneof![
                // Symbol-level topics
                (any::<u8>(), any::<u8>())
                    .prop_map(|(ch, sym)| { format!("channel{}.SYMBOL{}", ch % 5, sym % 10) }),
                // Channel-level topics (no symbol)
                any::<u8>().prop_map(|ch| format!("channel{}", ch % 5)),
            ]
        }

        // Strategy for generating random operations
        fn operation_strategy() -> impl Strategy<Value = Operation> {
            topic_strategy().prop_flat_map(|topic| {
                prop_oneof![
                    Just(Operation::MarkSubscribe(topic.clone())),
                    Just(Operation::ConfirmSubscribe(topic.clone())),
                    Just(Operation::MarkUnsubscribe(topic.clone())),
                    Just(Operation::ConfirmUnsubscribe(topic.clone())),
                    Just(Operation::MarkFailure(topic.clone())),
                    Just(Operation::AddReference(topic.clone())),
                    Just(Operation::RemoveReference(topic)),
                    Just(Operation::Clear),
                ]
            })
        }

        // Apply an operation to the state
        fn apply_operation(state: &SubscriptionState, op: &Operation) {
            match op {
                Operation::MarkSubscribe(topic) => state.mark_subscribe(topic),
                Operation::ConfirmSubscribe(topic) => state.confirm_subscribe(topic),
                Operation::MarkUnsubscribe(topic) => state.mark_unsubscribe(topic),
                Operation::ConfirmUnsubscribe(topic) => state.confirm_unsubscribe(topic),
                Operation::MarkFailure(topic) => state.mark_failure(topic),
                Operation::AddReference(topic) => {
                    state.add_reference(topic);
                }
                Operation::RemoveReference(topic) => {
                    state.remove_reference(topic);
                }
                Operation::Clear => state.clear(),
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(500))]

            /// Property: Invariants hold after any sequence of operations.
            #[rstest]
            fn prop_invariants_hold_after_operations(
                operations in prop::collection::vec(operation_strategy(), 1..50)
            ) {
                let state = SubscriptionState::new('.');

                // Apply all operations
                for (i, op) in operations.iter().enumerate() {
                    apply_operation(&state, op);

                    // Check invariants after each operation
                    check_invariants(&state, &format!("After op {i}: {op:?}"));
                }

                // Final invariant check
                check_invariants(&state, "Final state");
            }

            /// Property: Reference counting is always consistent.
            #[rstest]
            fn prop_reference_counting_consistency(
                ops in prop::collection::vec(
                    topic_strategy().prop_flat_map(|t| {
                        prop_oneof![
                            Just(Operation::AddReference(t.clone())),
                            Just(Operation::RemoveReference(t)),
                        ]
                    }),
                    1..100
                )
            ) {
                let state = SubscriptionState::new('.');

                for op in &ops {
                    apply_operation(&state, op);

                    // All reference counts must be >= 0 (NonZeroUsize or absent)
                    for entry in state.reference_counts.iter() {
                        assert!(entry.value().get() > 0);
                    }
                }
            }

            /// Property: all_topics() always equals confirmed ∪ pending_subscribe.
            #[rstest]
            fn prop_all_topics_is_union(
                operations in prop::collection::vec(operation_strategy(), 1..50)
            ) {
                let state = SubscriptionState::new('.');

                for op in &operations {
                    apply_operation(&state, op);

                    // Verify all_topics() == confirmed ∪ pending_subscribe
                    let all_topics: AHashSet<String> = state.all_topics().into_iter().collect();
                    let confirmed: AHashSet<String> = state.topics_from_map(&state.confirmed).into_iter().collect();
                    let pending_sub: AHashSet<String> = state.pending_subscribe_topics().into_iter().collect();
                    let expected: AHashSet<String> = confirmed.union(&pending_sub).cloned().collect();

                    assert_eq!(all_topics, expected);

                    // Ensure pending_unsubscribe topics are NOT in all_topics
                    let pending_unsub: AHashSet<String> = state.pending_unsubscribe_topics().into_iter().collect();
                    for topic in pending_unsub {
                        assert!(!all_topics.contains(&topic));
                    }
                }
            }

            /// Property: clear() resets to empty state.
            #[rstest]
            fn prop_clear_resets_completely(
                operations in prop::collection::vec(operation_strategy(), 1..30)
            ) {
                let state = SubscriptionState::new('.');

                // Apply random operations
                for op in &operations {
                    apply_operation(&state, op);
                }

                // Clear and verify complete reset
                state.clear();

                assert!(state.is_empty());
                assert_eq!(state.len(), 0);
                assert!(state.all_topics().is_empty());
                assert!(state.pending_subscribe_topics().is_empty());
                assert!(state.pending_unsubscribe_topics().is_empty());
                assert!(state.confirmed.is_empty());
                assert!(state.pending_subscribe.is_empty());
                assert!(state.pending_unsubscribe.is_empty());
                assert!(state.reference_counts.is_empty());
            }

            /// Property: Topics are mutually exclusive across states.
            #[rstest]
            fn prop_topic_mutual_exclusivity(
                operations in prop::collection::vec(operation_strategy(), 1..50),
                topic in topic_strategy()
            ) {
                let state = SubscriptionState::new('.');

                for (i, op) in operations.iter().enumerate() {
                    apply_operation(&state, op);
                    check_topic_exclusivity(&state, &topic, &format!("After op {i}: {op:?}"));
                }
            }
        }
    }

    #[rstest]
    fn test_exhaustive_two_step_transitions() {
        let operations = [
            "mark_subscribe",
            "confirm_subscribe",
            "mark_unsubscribe",
            "confirm_unsubscribe",
            "mark_failure",
        ];

        for &op1 in &operations {
            for &op2 in &operations {
                let state = SubscriptionState::new('.');
                let topic = "test.TOPIC";

                // Apply two operations
                apply_op(&state, op1, topic);
                apply_op(&state, op2, topic);

                // Verify invariants hold
                check_invariants(&state, &format!("{op1} → {op2}"));
                check_topic_exclusivity(&state, topic, &format!("{op1} → {op2}"));
            }
        }
    }

    fn apply_op(state: &SubscriptionState, op: &str, topic: &str) {
        match op {
            "mark_subscribe" => state.mark_subscribe(topic),
            "confirm_subscribe" => state.confirm_subscribe(topic),
            "mark_unsubscribe" => state.mark_unsubscribe(topic),
            "confirm_unsubscribe" => state.confirm_unsubscribe(topic),
            "mark_failure" => state.mark_failure(topic),
            _ => panic!("Unknown operation: {op}"),
        }
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_stress_rapid_resubscribe_pattern() {
        // Stress test the race condition we fixed: rapid unsubscribe → resubscribe
        let state = Arc::new(SubscriptionState::new('.'));
        let mut handles = vec![];

        for i in 0..100 {
            let state_clone = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                let topic = format!("rapid.SYMBOL{}", i % 10); // 10 unique topics, lots of contention

                // Initial subscribe
                state_clone.mark_subscribe(&topic);
                state_clone.confirm_subscribe(&topic);

                // Rapid unsubscribe → resubscribe (race condition scenario)
                state_clone.mark_unsubscribe(&topic);
                // Immediately resubscribe before unsubscribe ACK
                state_clone.mark_subscribe(&topic);
                // Now unsubscribe ACK arrives
                state_clone.confirm_unsubscribe(&topic);
                // Subscribe ACK arrives
                state_clone.confirm_subscribe(&topic);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        check_invariants(&state, "After rapid resubscribe stress test");
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_stress_failure_recovery_loop() {
        // Stress test failure → recovery loops
        // Each task gets its own unique topic to avoid race conditions in the test itself
        let state = Arc::new(SubscriptionState::new('.'));
        let mut handles = vec![];

        for i in 0..30 {
            let state_clone = Arc::clone(&state);
            let handle = tokio::spawn(async move {
                let topic = format!("failure.SYMBOL{i}"); // Unique topic per task

                // Subscribe and confirm
                state_clone.mark_subscribe(&topic);
                state_clone.confirm_subscribe(&topic);

                // Simulate multiple failures and recoveries
                for _ in 0..5 {
                    state_clone.mark_failure(&topic);
                    state_clone.confirm_subscribe(&topic); // Re-confirm after retry
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        check_invariants(&state, "After failure recovery loops");

        // All should eventually be confirmed (30 unique topics)
        assert_eq!(state.len(), 30);
    }
}

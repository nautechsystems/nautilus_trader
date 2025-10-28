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

//! Subscription tracking helpers for the Bybit WebSocket client.
//!
//! These utilities maintain the confirmed/pending topic sets and expose shared
//! topic-splitting primitives for the client and handler.

use std::{num::NonZeroUsize, sync::Arc};

use ahash::AHashSet;
use dashmap::DashMap;
use ustr::Ustr;

pub(crate) fn split_topic(topic: &str) -> (&str, Option<&str>) {
    topic
        .split_once('.')
        .map_or((topic, None), |(channel, symbol)| (channel, Some(symbol)))
}

pub(crate) fn track_topic(
    map: &DashMap<Ustr, AHashSet<Ustr>>,
    channel: &str,
    symbol: Option<&str>,
) {
    let channel_ustr = Ustr::from(channel);
    if let Some(symbol) = symbol {
        let mut entry = map.entry(channel_ustr).or_default();
        entry.insert(Ustr::from(symbol));
    } else {
        map.entry(channel_ustr).or_default();
    }
}

pub(crate) fn untrack_topic(
    map: &DashMap<Ustr, AHashSet<Ustr>>,
    channel: &str,
    symbol: Option<&str>,
) {
    let channel_ustr = Ustr::from(channel);
    if let Some(symbol) = symbol {
        let symbol_ustr = Ustr::from(symbol);
        let mut remove_channel = false;
        if let Some(mut entry) = map.get_mut(&channel_ustr) {
            entry.remove(&symbol_ustr);
            remove_channel = entry.is_empty();
        }
        if remove_channel {
            map.remove(&channel_ustr);
        }
    } else {
        map.remove(&channel_ustr);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SubscriptionState {
    confirmed: Arc<DashMap<Ustr, AHashSet<Ustr>>>,
    pending_subscribe: Arc<DashMap<Ustr, AHashSet<Ustr>>>,
    pending_unsubscribe: Arc<DashMap<Ustr, AHashSet<Ustr>>>,
    reference_counts: Arc<DashMap<Ustr, NonZeroUsize>>,
}

impl SubscriptionState {
    pub(crate) fn new() -> Self {
        Self {
            confirmed: Arc::new(DashMap::new()),
            pending_subscribe: Arc::new(DashMap::new()),
            pending_unsubscribe: Arc::new(DashMap::new()),
            reference_counts: Arc::new(DashMap::new()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn confirmed(&self) -> Arc<DashMap<Ustr, AHashSet<Ustr>>> {
        Arc::clone(&self.confirmed)
    }

    #[allow(dead_code)]
    pub(crate) fn pending(&self) -> Arc<DashMap<Ustr, AHashSet<Ustr>>> {
        // For backward compatibility, return pending_subscribe
        Arc::clone(&self.pending_subscribe)
    }

    pub(crate) fn len(&self) -> usize {
        // Count ONLY confirmed subscriptions for public API (matches BitMEX and OKX)
        self.confirmed
            .iter()
            .map(|entry| {
                let symbols = entry.value();
                if symbols.is_empty() { 1 } else { symbols.len() }
            })
            .sum()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.confirmed.is_empty()
            && self.pending_subscribe.is_empty()
            && self.pending_unsubscribe.is_empty()
    }

    pub(crate) fn mark_subscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        track_topic(&self.pending_subscribe, channel, symbol);
    }

    pub(crate) fn mark_unsubscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        track_topic(&self.pending_unsubscribe, channel, symbol);
        untrack_topic(&self.confirmed, channel, symbol);
    }

    #[allow(dead_code)]
    pub(crate) fn confirm_subscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        untrack_topic(&self.pending_subscribe, channel, symbol);
        track_topic(&self.confirmed, channel, symbol);
    }

    #[allow(dead_code)]
    pub(crate) fn confirm_unsubscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        untrack_topic(&self.pending_unsubscribe, channel, symbol);
    }

    #[allow(dead_code)]
    pub(crate) fn mark_failure(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        untrack_topic(&self.confirmed, channel, symbol);
        track_topic(&self.pending_subscribe, channel, symbol);
    }

    pub(crate) fn pending_subscribe_topics(&self) -> Vec<String> {
        let mut topics = Vec::new();
        for entry in self.pending_subscribe.iter() {
            let channel = entry.key();
            let symbols = entry.value();
            if symbols.is_empty() {
                topics.push(channel.to_string());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel.as_str(), symbol.as_str()));
                }
            }
        }
        topics
    }

    pub(crate) fn pending_unsubscribe_topics(&self) -> Vec<String> {
        let mut topics = Vec::new();
        for entry in self.pending_unsubscribe.iter() {
            let channel = entry.key();
            let symbols = entry.value();
            if symbols.is_empty() {
                topics.push(channel.to_string());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel.as_str(), symbol.as_str()));
                }
            }
        }
        topics
    }

    pub(crate) fn all_topics(&self) -> Vec<String> {
        let mut topics = Vec::new();

        // Collect from confirmed
        for entry in self.confirmed.iter() {
            let channel = entry.key();
            let symbols = entry.value();
            if symbols.is_empty() {
                topics.push(channel.to_string());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel.as_str(), symbol.as_str()));
                }
            }
        }

        // Collect from pending_subscribe (for retry on reconnect)
        for entry in self.pending_subscribe.iter() {
            let channel = entry.key();
            let symbols = entry.value();
            if symbols.is_empty() {
                topics.push(channel.to_string());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel.as_str(), symbol.as_str()));
                }
            }
        }

        // Do NOT include pending_unsubscribe - we don't want to resubscribe to topics being removed

        topics
    }

    /// Increments the reference count for a topic.
    ///
    /// Returns `true` if this is the first subscription (should send subscribe message to Bybit).
    pub(crate) fn add_reference(&self, topic: &str) -> bool {
        let mut should_subscribe = false;
        let topic_ustr = Ustr::from(topic);

        self.reference_counts
            .entry(topic_ustr)
            .and_modify(|count| {
                // Increment existing count
                *count = NonZeroUsize::new(count.get() + 1).expect("reference count overflow");
            })
            .or_insert_with(|| {
                // First subscription
                should_subscribe = true;
                NonZeroUsize::new(1).expect("NonZeroUsize::new(1) should never fail")
            });

        should_subscribe
    }

    /// Decrements the reference count for a topic.
    ///
    /// Returns `true` if this was the last subscription (should send unsubscribe message to Bybit).
    pub(crate) fn remove_reference(&self, topic: &str) -> bool {
        let topic_ustr = Ustr::from(topic);
        if let Some(mut entry) = self.reference_counts.get_mut(&topic_ustr) {
            let current = entry.get();

            if current == 1 {
                // Last reference - remove and signal to unsubscribe
                drop(entry); // Drop the mutable reference before removing
                self.reference_counts.remove(&topic_ustr);
                return true;
            }

            // Decrement count
            *entry = NonZeroUsize::new(current - 1)
                .expect("reference count should never reach zero here");
        }

        false
    }

    /// Returns the current reference count for a topic.
    #[allow(dead_code)]
    pub(crate) fn get_reference_count(&self, topic: &str) -> usize {
        let topic_ustr = Ustr::from(topic);
        self.reference_counts
            .get(&topic_ustr)
            .map_or(0, |count| count.get())
    }
}

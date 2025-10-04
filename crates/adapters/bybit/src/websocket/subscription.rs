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

use std::sync::Arc;

use ahash::AHashSet;
use dashmap::DashMap;
use ustr::Ustr;

pub(crate) fn split_topic(topic: &str) -> (&str, Option<&str>) {
    topic
        .split_once('.')
        .map_or((topic, None), |(channel, symbol)| (channel, Some(symbol)))
}

pub(crate) fn track_topic(
    map: &DashMap<String, AHashSet<Ustr>>,
    channel: &str,
    symbol: Option<&str>,
) {
    if let Some(symbol) = symbol {
        let mut entry = map.entry(channel.to_string()).or_default();
        entry.insert(Ustr::from(symbol));
    } else {
        map.entry(channel.to_string()).or_default();
    }
}

pub(crate) fn untrack_topic(
    map: &DashMap<String, AHashSet<Ustr>>,
    channel: &str,
    symbol: Option<&str>,
) {
    if let Some(symbol) = symbol {
        let symbol_ustr = Ustr::from(symbol);
        let mut remove_channel = false;
        if let Some(mut entry) = map.get_mut(channel) {
            entry.remove(&symbol_ustr);
            remove_channel = entry.is_empty();
        }
        if remove_channel {
            map.remove(channel);
        }
    } else {
        map.remove(channel);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SubscriptionState {
    confirmed: Arc<DashMap<String, AHashSet<Ustr>>>,
    pending_subscribe: Arc<DashMap<String, AHashSet<Ustr>>>,
    pending_unsubscribe: Arc<DashMap<String, AHashSet<Ustr>>>,
}

impl SubscriptionState {
    pub(crate) fn new() -> Self {
        Self {
            confirmed: Arc::new(DashMap::new()),
            pending_subscribe: Arc::new(DashMap::new()),
            pending_unsubscribe: Arc::new(DashMap::new()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn confirmed(&self) -> Arc<DashMap<String, AHashSet<Ustr>>> {
        Arc::clone(&self.confirmed)
    }

    #[allow(dead_code)]
    pub(crate) fn pending(&self) -> Arc<DashMap<String, AHashSet<Ustr>>> {
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
                topics.push(channel.clone());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel, symbol.as_str()));
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
                topics.push(channel.clone());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel, symbol.as_str()));
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
                topics.push(channel.clone());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel, symbol.as_str()));
                }
            }
        }

        // Collect from pending_subscribe (for retry on reconnect)
        for entry in self.pending_subscribe.iter() {
            let channel = entry.key();
            let symbols = entry.value();
            if symbols.is_empty() {
                topics.push(channel.clone());
            } else {
                for symbol in symbols.iter() {
                    topics.push(format!("{}.{}", channel, symbol.as_str()));
                }
            }
        }

        // Do NOT include pending_unsubscribe - we don't want to resubscribe to topics being removed

        topics
    }
}

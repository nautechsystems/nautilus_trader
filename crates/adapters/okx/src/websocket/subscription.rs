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

//! Subscription tracking helpers for the OKX WebSocket client.

use std::sync::Arc;

use ahash::AHashSet;
use dashmap::DashMap;
use ustr::Ustr;

use crate::{
    common::enums::OKXInstrumentType,
    websocket::{
        enums::OKXWsChannel,
        messages::{OKXSubscriptionArg, OKXWebSocketArg},
    },
};

fn topic_from_parts(
    channel: &OKXWsChannel,
    inst_id: Option<&Ustr>,
    inst_family: Option<&Ustr>,
    inst_type: Option<&OKXInstrumentType>,
    bar: Option<&Ustr>,
) -> String {
    let base = channel.as_ref();

    if let Some(inst_id) = inst_id {
        let inst_id = inst_id.as_str();
        if let Some(bar) = bar {
            format!("{base}:{inst_id}:{}", bar.as_str())
        } else {
            format!("{base}:{inst_id}")
        }
    } else if let Some(inst_family) = inst_family {
        format!("{base}:{}", inst_family.as_str())
    } else if let Some(inst_type) = inst_type {
        format!("{base}:{}", inst_type.as_ref())
    } else {
        base.to_string()
    }
}

pub(crate) fn topic_from_subscription_arg(arg: &OKXSubscriptionArg) -> String {
    topic_from_parts(
        &arg.channel,
        arg.inst_id.as_ref(),
        arg.inst_family.as_ref(),
        arg.inst_type.as_ref(),
        None,
    )
}

pub(crate) fn topic_from_websocket_arg(arg: &OKXWebSocketArg) -> String {
    topic_from_parts(
        &arg.channel,
        arg.inst_id.as_ref(),
        arg.inst_family.as_ref(),
        arg.inst_type.as_ref(),
        arg.bar.as_ref(),
    )
}

pub(crate) fn split_topic(topic: &str) -> (&str, Option<&str>) {
    topic
        .split_once(':')
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
    pending: Arc<DashMap<String, AHashSet<Ustr>>>,
}

impl SubscriptionState {
    pub(crate) fn new() -> Self {
        Self {
            confirmed: Arc::new(DashMap::new()),
            pending: Arc::new(DashMap::new()),
        }
    }

    pub(crate) fn confirmed(&self) -> Arc<DashMap<String, AHashSet<Ustr>>> {
        Arc::clone(&self.confirmed)
    }

    pub(crate) fn pending(&self) -> Arc<DashMap<String, AHashSet<Ustr>>> {
        Arc::clone(&self.pending)
    }

    pub(crate) fn len(&self) -> usize {
        self.confirmed.len()
    }

    pub(crate) fn mark_subscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        track_topic(&self.pending, channel, symbol);
    }

    pub(crate) fn mark_unsubscribe(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        track_topic(&self.pending, channel, symbol);
        untrack_topic(&self.confirmed, channel, symbol);
    }

    pub(crate) fn confirm(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        untrack_topic(&self.pending, channel, symbol);
        track_topic(&self.confirmed, channel, symbol);
    }

    pub(crate) fn mark_failure(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        untrack_topic(&self.confirmed, channel, symbol);
        track_topic(&self.pending, channel, symbol);
    }

    pub(crate) fn clear_pending(&self, topic: &str) {
        let (channel, symbol) = split_topic(topic);
        untrack_topic(&self.pending, channel, symbol);
    }
}

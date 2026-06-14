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

use dashmap::DashMap;
use nautilus_common::messages::DataEvent;
use nautilus_model::{data::Data, identifiers::InstrumentId};

use crate::websocket::{
    client::LighterWebSocketClient,
    error::LighterWsError,
    messages::{LighterWsChannel, NautilusWsMessage},
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum MarketStatsKind {
    MarkPrice,
    IndexPrice,
    FundingRate,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MarketStatsFlags {
    pub(super) mark_price: bool,
    pub(super) index_price: bool,
    pub(super) funding_rate: bool,
}

impl MarketStatsFlags {
    pub(super) fn is_empty(self) -> bool {
        !self.mark_price && !self.index_price && !self.funding_rate
    }

    pub(super) fn contains(self, kind: MarketStatsKind) -> bool {
        match kind {
            MarketStatsKind::MarkPrice => self.mark_price,
            MarketStatsKind::IndexPrice => self.index_price,
            MarketStatsKind::FundingRate => self.funding_rate,
        }
    }

    pub(super) fn insert(&mut self, kind: MarketStatsKind) {
        match kind {
            MarketStatsKind::MarkPrice => self.mark_price = true,
            MarketStatsKind::IndexPrice => self.index_price = true,
            MarketStatsKind::FundingRate => self.funding_rate = true,
        }
    }

    pub(super) fn remove(&mut self, kind: MarketStatsKind) {
        match kind {
            MarketStatsKind::MarkPrice => self.mark_price = false,
            MarketStatsKind::IndexPrice => self.index_price = false,
            MarketStatsKind::FundingRate => self.funding_rate = false,
        }
    }
}

impl From<MarketStatsKind> for MarketStatsFlags {
    fn from(kind: MarketStatsKind) -> Self {
        let mut flags = Self::default();
        flags.insert(kind);
        flags
    }
}

#[derive(Debug, Clone)]
pub(super) struct MarketStatsSubscription {
    pub(super) channel: LighterWsChannel,
    pub(super) flags: MarketStatsFlags,
}

impl MarketStatsSubscription {
    pub(super) fn new(channel: LighterWsChannel, kind: MarketStatsKind) -> Self {
        Self {
            channel,
            flags: kind.into(),
        }
    }
}

pub(super) async fn subscribe_channel(
    ws: LighterWebSocketClient,
    channel: LighterWsChannel,
) -> Result<(), LighterWsError> {
    match channel {
        LighterWsChannel::MarketStats(selection) => ws.subscribe_market_stats(selection).await,
        LighterWsChannel::SpotMarketStats(selection) => {
            ws.subscribe_spot_market_stats(selection).await
        }
        _ => unreachable!("market-stats subscription called with non-market-stats channel"),
    }
}

pub(super) async fn unsubscribe_channel(
    ws: LighterWebSocketClient,
    channel: LighterWsChannel,
) -> Result<(), LighterWsError> {
    match channel {
        LighterWsChannel::MarketStats(selection) => ws.unsubscribe_market_stats(selection).await,
        LighterWsChannel::SpotMarketStats(selection) => {
            ws.unsubscribe_spot_market_stats(selection).await
        }
        _ => unreachable!("market-stats unsubscription called with non-market-stats channel"),
    }
}

pub(super) fn emit_ws_message(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    subscriptions: &DashMap<InstrumentId, MarketStatsSubscription>,
    message: &NautilusWsMessage,
) -> bool {
    match message {
        NautilusWsMessage::MarkPrice(mark_price) => {
            if !is_subscribed(
                subscriptions,
                &mark_price.instrument_id,
                MarketStatsKind::MarkPrice,
            ) {
                return false;
            }

            if let Err(e) = sender.send(DataEvent::Data(Data::MarkPriceUpdate(*mark_price))) {
                log::error!("Failed to send mark price: {e}");
            }
            true
        }
        NautilusWsMessage::IndexPrice(index_price) => {
            if !is_subscribed(
                subscriptions,
                &index_price.instrument_id,
                MarketStatsKind::IndexPrice,
            ) {
                return false;
            }

            if let Err(e) = sender.send(DataEvent::Data(Data::IndexPriceUpdate(*index_price))) {
                log::error!("Failed to send index price: {e}");
            }
            true
        }
        NautilusWsMessage::FundingRate(funding_rate) => {
            if !is_subscribed(
                subscriptions,
                &funding_rate.instrument_id,
                MarketStatsKind::FundingRate,
            ) {
                return false;
            }

            if let Err(e) = sender.send(DataEvent::FundingRate(*funding_rate)) {
                log::error!("Failed to send funding rate: {e}");
            }
            true
        }
        _ => false,
    }
}

fn is_subscribed(
    subscriptions: &DashMap<InstrumentId, MarketStatsSubscription>,
    instrument_id: &InstrumentId,
    kind: MarketStatsKind,
) -> bool {
    subscriptions
        .get(instrument_id)
        .is_some_and(|subscription| subscription.flags.contains(kind))
}

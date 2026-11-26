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

//! Live market data client implementations for the Kraken adapter.
//!
//! This module provides separate data clients for Kraken Spot and Futures markets:
//!
//! - [`KrakenSpotDataClient`] - For Spot markets using WebSocket v2
//! - [`KrakenFuturesDataClient`] - For Futures markets
//!
//! # Supported Data Types
//!
//! ## Spot
//! - Order book deltas and snapshots
//! - Trade ticks
//! - Quote ticks (best bid/ask)
//! - OHLC bars
//!
//! ## Futures
//! - Order book deltas and snapshots
//! - Trade ticks
//! - Quote ticks (best bid/ask)
//! - Mark prices
//! - Index prices

use std::time::Duration;

use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

mod futures;
mod spot;

pub use futures::KrakenFuturesDataClient;
pub use spot::KrakenSpotDataClient;

fn instrument_refresh_interval() -> tokio::time::Interval {
    let period = Duration::from_secs(60);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

fn apply_instrument_definitions(
    cache_http: impl FnOnce(&[InstrumentAny]),
    cache: &AtomicMap<InstrumentId, InstrumentAny>,
    sender: &EventSender<DataEvent>,
    instruments: &[InstrumentAny],
    publish_unchanged: bool,
) {
    let current = cache.load();
    let mut accepted = Vec::new();
    let mut published = Vec::new();

    for instrument in instruments {
        let previous = current.get(&instrument.id());
        if previous.is_some_and(|previous| previous.ts_init() > instrument.ts_init()) {
            continue;
        }

        if publish_unchanged
            || previous.is_none_or(|previous| instrument_definition_changed(previous, instrument))
        {
            published.push(instrument);
        }

        accepted.push(instrument.clone());
    }

    if accepted.is_empty() {
        return;
    }

    // An unchanged snapshot still advances freshness against overlapping requests
    cache.rcu(|map| {
        for instrument in &accepted {
            map.insert(instrument.id(), instrument.clone());
        }
    });

    cache_http(&accepted);

    for instrument in published {
        if let Err(e) = sender.send(DataEvent::Data(instrument.clone().into())) {
            log::error!("Failed to send instrument: {e}");
        }
    }
}

fn instrument_definition_changed(previous: &InstrumentAny, current: &InstrumentAny) -> bool {
    previous.price_precision() != current.price_precision()
        || previous.size_precision() != current.size_precision()
        || previous.price_increment() != current.price_increment()
        || previous.size_increment() != current.size_increment()
        || previous.multiplier() != current.multiplier()
        || previous.margin_init() != current.margin_init()
        || previous.margin_maint() != current.margin_maint()
        || previous.base_currency() != current.base_currency()
        || previous.quote_currency() != current.quote_currency()
        || previous.settlement_currency() != current.settlement_currency()
        || previous.is_inverse() != current.is_inverse()
        || previous.min_quantity() != current.min_quantity()
        || previous.raw_symbol() != current.raw_symbol()
        || previous.info() != current.info()
}

#[cfg(test)]
pub(crate) mod tests {
    #[allow(unused_imports)]
    pub(crate) use super::futures::tests::benchmarks::{bench_quotes, bench_snapshots};
}

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

//! Quote cache for reconstructing Bybit WebSocket partial updates.

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::quote::QuoteTick,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use super::messages::{BybitWsTickerLinear, BybitWsTickerOption};
use crate::common::parse::{parse_price_with_precision, parse_quantity_with_precision};

/// Maintains quote state for each instrument to handle partial quote updates.
///
/// Bybit ticker messages may contain incomplete information (missing bid or ask side).
/// When this happens, we need to reference the last known complete quote to construct
/// a valid `QuoteTick` which requires both sides.
#[derive(Debug)]
pub struct QuoteCache {
    last_quotes: AHashMap<InstrumentId, QuoteTick>,
}

impl QuoteCache {
    /// Creates a new [`QuoteCache`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_quotes: AHashMap::new(),
        }
    }

    /// Clears all cached quotes, typically used after reconnection.
    pub fn clear(&mut self) {
        self.last_quotes.clear();
    }

    /// Processes an incoming linear ticker message, emitting a complete quote when possible.
    pub fn process_linear_ticker(
        &mut self,
        data: &BybitWsTickerLinear,
        instrument_id: InstrumentId,
        instrument: &InstrumentAny,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<QuoteTick> {
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let bid_price = data
            .bid1_price
            .as_deref()
            .map(|s| parse_price_with_precision(s, price_precision, "bid1Price"))
            .transpose()?;

        let ask_price = data
            .ask1_price
            .as_deref()
            .map(|s| parse_price_with_precision(s, price_precision, "ask1Price"))
            .transpose()?;

        let bid_size = data
            .bid1_size
            .as_deref()
            .map(|s| parse_quantity_with_precision(s, size_precision, "bid1Size"))
            .transpose()?;

        let ask_size = data
            .ask1_size
            .as_deref()
            .map(|s| parse_quantity_with_precision(s, size_precision, "ask1Size"))
            .transpose()?;

        let cached = self.last_quotes.get(&instrument_id);

        let bid_price = match (bid_price, cached) {
            (Some(p), _) => p,
            (None, Some(q)) => q.bid_price,
            (None, None) => {
                anyhow::bail!("Bybit ticker missing bid1Price and no cached value available")
            }
        };

        let ask_price = match (ask_price, cached) {
            (Some(p), _) => p,
            (None, Some(q)) => q.ask_price,
            (None, None) => {
                anyhow::bail!("Bybit ticker missing ask1Price and no cached value available")
            }
        };

        let bid_size = match (bid_size, cached) {
            (Some(s), _) => s,
            (None, Some(q)) => q.bid_size,
            (None, None) => {
                anyhow::bail!("Bybit ticker missing bid1Size and no cached value available")
            }
        };

        let ask_size = match (ask_size, cached) {
            (Some(s), _) => s,
            (None, Some(q)) => q.ask_size,
            (None, None) => {
                anyhow::bail!("Bybit ticker missing ask1Size and no cached value available")
            }
        };

        let quote = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );

        self.last_quotes.insert(instrument_id, quote);

        Ok(quote)
    }

    /// Processes an incoming option ticker message, emitting a complete quote when possible.
    pub fn process_option_ticker(
        &mut self,
        data: &BybitWsTickerOption,
        instrument_id: InstrumentId,
        instrument: &InstrumentAny,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<QuoteTick> {
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let bid_price = parse_price_with_precision(&data.bid_price, price_precision, "bidPrice")?;
        let ask_price = parse_price_with_precision(&data.ask_price, price_precision, "askPrice")?;
        let bid_size = parse_quantity_with_precision(&data.bid_size, size_precision, "bidSize")?;
        let ask_size = parse_quantity_with_precision(&data.ask_size, size_precision, "askSize")?;

        let quote = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );

        // Update cache
        self.last_quotes.insert(instrument_id, quote);

        Ok(quote)
    }
}

impl Default for QuoteCache {
    fn default() -> Self {
        Self::new()
    }
}

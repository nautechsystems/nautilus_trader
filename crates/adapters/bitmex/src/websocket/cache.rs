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

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{data::quote::QuoteTick, identifiers::InstrumentId, types::price::Price};

use super::{
    messages::BitmexQuoteMsg,
    parse::{parse_quantity, parse_quote_msg},
};
use crate::common::parse::parse_instrument_id;

/// Maintains quote state for each instrument to handle partial quote updates.
///
/// BitMEX quote messages may contain incomplete information (missing bid or ask side).
/// When this happens, we need to reference the last known complete quote to construct
/// a valid `QuoteTick` which requires both sides.
pub(crate) struct QuoteCache {
    last_quotes: AHashMap<InstrumentId, QuoteTick>,
}

impl QuoteCache {
    /// Creates a new [`QuoteCache`] instance.
    pub fn new() -> Self {
        Self {
            last_quotes: AHashMap::new(),
        }
    }

    pub fn process(
        &mut self,
        msg: &BitmexQuoteMsg,
        price_precision: u8,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        let instrument_id = parse_instrument_id(&msg.symbol);

        let quote = if let Some(last_quote) = self.last_quotes.get(&instrument_id) {
            Some(parse_quote_msg(msg, last_quote, price_precision, ts_init))
        } else {
            match (msg.bid_price, msg.ask_price, msg.bid_size, msg.ask_size) {
                (Some(bid_price), Some(ask_price), Some(bid_size), Some(ask_size)) => {
                    Some(QuoteTick::new(
                        instrument_id,
                        Price::new(bid_price, price_precision),
                        Price::new(ask_price, price_precision),
                        parse_quantity(bid_size),
                        parse_quantity(ask_size),
                        UnixNanos::from(msg.timestamp),
                        ts_init,
                    ))
                }
                _ => None,
            }
        };

        // Update cache if a quote was created
        if let Some(quote) = &quote {
            self.last_quotes.insert(instrument_id, *quote);
        }

        quote
    }
}

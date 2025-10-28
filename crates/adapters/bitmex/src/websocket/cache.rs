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

//! Quote cache for reconstructing BitMEX WebSocket partial updates.

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::quote::QuoteTick,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::price::Price,
};

use super::{messages::BitmexQuoteMsg, parse::parse_quote_msg};
use crate::common::parse::parse_contracts_quantity;

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

    /// Clears all cached quotes, typically used after reconnection.
    pub fn clear(&mut self) {
        self.last_quotes.clear();
    }

    /// Processes an incoming quote message, emitting a complete quote when possible.
    pub fn process(
        &mut self,
        msg: &BitmexQuoteMsg,
        instrument: &InstrumentAny,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();

        let quote = if let Some(last_quote) = self.last_quotes.get(&instrument_id) {
            Some(parse_quote_msg(
                msg,
                last_quote,
                instrument,
                instrument_id,
                price_precision,
                ts_init,
            ))
        } else {
            match (msg.bid_price, msg.ask_price, msg.bid_size, msg.ask_size) {
                (Some(bid_price), Some(ask_price), Some(bid_size), Some(ask_size)) => {
                    Some(QuoteTick::new(
                        instrument_id,
                        Price::new(bid_price, price_precision),
                        Price::new(ask_price, price_precision),
                        parse_contracts_quantity(bid_size, instrument),
                        parse_contracts_quantity(ask_size, instrument),
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use nautilus_model::{
        identifiers::Symbol,
        instruments::currency_pair::CurrencyPair,
        types::{Currency, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::parse::parse_instrument_id;

    fn make_test_instrument(price_precision: u8, size_precision: u8) -> InstrumentAny {
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            price_precision,
            size_precision,
            Price::new(1.0, price_precision),
            Quantity::new(1.0, size_precision),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_quote_cache_new() {
        let cache = QuoteCache::new();
        assert!(cache.last_quotes.is_empty());
    }

    #[rstest]
    fn test_process_complete_quote() {
        let mut cache = QuoteCache::new();

        let msg = BitmexQuoteMsg {
            symbol: "XBTUSD".into(),
            bid_price: Some(50000.5),
            ask_price: Some(50001.0),
            bid_size: Some(100),
            ask_size: Some(150),
            timestamp: Utc::now(),
        };

        let ts_init = UnixNanos::default();
        let instrument = make_test_instrument(1, 0);
        let quote = cache.process(&msg, &instrument, ts_init);

        assert!(quote.is_some());
        let quote = quote.unwrap();
        assert_eq!(quote.instrument_id, parse_instrument_id("XBTUSD".into()));
        assert_eq!(quote.bid_price, Price::new(50000.5, 1));
        assert_eq!(quote.ask_price, Price::new(50001.0, 1));
        assert_eq!(quote.bid_size, Quantity::from(100));
        assert_eq!(quote.ask_size, Quantity::from(150));
    }

    #[rstest]
    fn test_process_partial_quote_without_cache() {
        let mut cache = QuoteCache::new();

        // Partial quote with missing ask_size
        let msg = BitmexQuoteMsg {
            symbol: "XBTUSD".into(),
            bid_price: Some(50000.5),
            ask_price: Some(50001.0),
            bid_size: Some(100),
            ask_size: None,
            timestamp: Utc::now(),
        };

        let ts_init = UnixNanos::default();
        let instrument = make_test_instrument(1, 0);
        let quote = cache.process(&msg, &instrument, ts_init);

        // Should return None for incomplete first quote
        assert!(quote.is_none());
    }

    #[rstest]
    fn test_process_partial_quote_with_cache() {
        let mut cache = QuoteCache::new();

        // First, process a complete quote
        let complete_msg = BitmexQuoteMsg {
            symbol: "XBTUSD".into(),
            bid_price: Some(50000.5),
            ask_price: Some(50001.0),
            bid_size: Some(100),
            ask_size: Some(150),
            timestamp: Utc::now(),
        };

        let ts_init = UnixNanos::default();
        let instrument = make_test_instrument(1, 0);
        let first_quote = cache.process(&complete_msg, &instrument, ts_init).unwrap();

        // Now process a partial quote with only bid update
        let partial_msg = BitmexQuoteMsg {
            symbol: "XBTUSD".into(),
            bid_price: Some(50002.0),
            ask_price: None,
            bid_size: Some(200),
            ask_size: None,
            timestamp: Utc::now(),
        };

        let quote = cache.process(&partial_msg, &instrument, ts_init);

        assert!(quote.is_some());
        let quote = quote.unwrap();

        // Bid should be updated
        assert_eq!(quote.bid_price, Price::new(50002.0, 1));
        assert_eq!(quote.bid_size, Quantity::from(200));

        // Ask should be from the cached quote
        assert_eq!(quote.ask_price, first_quote.ask_price);
        assert_eq!(quote.ask_size, first_quote.ask_size);
    }

    #[rstest]
    fn test_cache_updates_after_processing() {
        let mut cache = QuoteCache::new();

        let msg = BitmexQuoteMsg {
            symbol: "XBTUSD".into(),
            bid_price: Some(50000.5),
            ask_price: Some(50001.0),
            bid_size: Some(100),
            ask_size: Some(150),
            timestamp: Utc::now(),
        };

        let ts_init = UnixNanos::default();
        let instrument = make_test_instrument(1, 0);
        let quote = cache.process(&msg, &instrument, ts_init).unwrap();

        let instrument_id = parse_instrument_id("XBTUSD".into());
        assert!(cache.last_quotes.contains_key(&instrument_id));
        assert_eq!(cache.last_quotes[&instrument_id], quote);
    }
}

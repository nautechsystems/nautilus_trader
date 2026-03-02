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

//! Reactive ATM (at-the-money) price tracker for option chain subscriptions.

use nautilus_model::{
    data::{
        IndexPriceUpdate, MarkPriceUpdate, QuoteTick,
        option_chain::{AtmSource, OptionGreeks},
    },
    types::Price,
};

/// Tracks the raw ATM price reactively from incoming market data events.
///
/// Does not interact with cache — receives updates via handler callbacks.
/// Closest-strike resolution is delegated to `StrikeRange::resolve()`.
#[derive(Debug)]
pub struct AtmTracker {
    source: AtmSource,
    atm_price: Option<Price>,
    /// Precision used when converting forward prices from f64 to Price.
    forward_precision: u8,
}

impl AtmTracker {
    /// Creates a new [`AtmTracker`] for the given ATM source.
    pub fn new(source: AtmSource) -> Self {
        Self {
            source,
            atm_price: None,
            forward_precision: 2,
        }
    }

    /// Sets the precision used when converting forward prices from f64 to Price.
    pub fn set_forward_precision(&mut self, precision: u8) {
        self.forward_precision = precision;
    }

    /// Returns the current raw ATM price (if available).
    #[must_use]
    pub fn atm_price(&self) -> Option<Price> {
        self.atm_price
    }

    /// Returns the configured ATM source.
    #[must_use]
    pub fn source(&self) -> &AtmSource {
        &self.source
    }

    /// Updates from a quote tick (for `UnderlyingQuoteMid` source).
    ///
    /// Returns `true` if the ATM price was updated.
    pub fn update_from_quote(&mut self, quote: &QuoteTick) -> bool {
        if let AtmSource::UnderlyingQuoteMid(id) = self.source
            && quote.instrument_id == id
        {
            let mid = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
            self.atm_price = Some(Price::new(mid, quote.bid_price.precision));
            return true;
        }
        false
    }

    /// Updates from a mark price event (for `MarkPrice` source).
    ///
    /// Returns `true` if the ATM price was updated.
    pub fn update_from_mark_price(&mut self, mark: &MarkPriceUpdate) -> bool {
        if let AtmSource::MarkPrice(id) = self.source
            && mark.instrument_id == id
        {
            self.atm_price = Some(mark.value);
            return true;
        }
        false
    }

    /// Sets the initial ATM price (e.g. from a forward price fetched via HTTP).
    ///
    /// This allows instant bootstrap without waiting for the first WebSocket tick.
    /// Subsequent live updates will overwrite this value normally.
    pub fn set_initial_price(&mut self, price: Price) {
        self.atm_price = Some(price);
    }

    /// Updates from an index price event (for `IndexPrice` source).
    ///
    /// Returns `true` if the ATM price was updated.
    pub fn update_from_index_price(&mut self, index: &IndexPriceUpdate) -> bool {
        if let AtmSource::IndexPrice(id) = self.source
            && index.instrument_id == id
        {
            self.atm_price = Some(index.value);
            return true;
        }
        false
    }

    /// Updates from an option greeks event (for `ForwardPrice` source).
    ///
    /// Extracts `underlying_price` from the greeks — the exchange-provided
    /// forward price for this expiry. Returns `true` if the ATM price was updated.
    pub fn update_from_option_greeks(&mut self, greeks: &OptionGreeks) -> bool {
        if self.source == AtmSource::ForwardPrice
            && let Some(fwd) = greeks.underlying_price
        {
            self.atm_price = Some(Price::new(fwd, self.forward_precision));
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::*;

    use super::*;

    fn btc_perp() -> InstrumentId {
        InstrumentId::from("BTC-PERPETUAL.DERIBIT")
    }

    fn eth_perp() -> InstrumentId {
        InstrumentId::from("ETH-PERPETUAL.DERIBIT")
    }

    #[rstest]
    fn test_atm_tracker_initial_none() {
        let tracker = AtmTracker::new(AtmSource::MarkPrice(btc_perp()));
        assert!(tracker.atm_price().is_none());
    }

    #[rstest]
    fn test_atm_tracker_update_from_quote_mid() {
        let mut tracker = AtmTracker::new(AtmSource::UnderlyingQuoteMid(btc_perp()));
        let quote = QuoteTick::new(
            btc_perp(),
            Price::from("50000.00"),
            Price::from("50100.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        );
        assert!(tracker.update_from_quote(&quote));
        let atm = tracker.atm_price().unwrap();
        // Mid = (50000.00 + 50100.00) / 2 = 50050.00
        assert_eq!(atm, Price::from("50050.00"));
    }

    #[rstest]
    fn test_atm_tracker_ignores_wrong_instrument() {
        let mut tracker = AtmTracker::new(AtmSource::UnderlyingQuoteMid(btc_perp()));
        let quote = QuoteTick::new(
            eth_perp(),
            Price::from("3000.00"),
            Price::from("3001.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        );
        assert!(!tracker.update_from_quote(&quote));
        assert!(tracker.atm_price().is_none());
    }

    #[rstest]
    fn test_atm_tracker_update_from_mark_price() {
        let mut tracker = AtmTracker::new(AtmSource::MarkPrice(btc_perp()));
        let mark = MarkPriceUpdate {
            instrument_id: btc_perp(),
            value: Price::from("50500.00"),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };
        assert!(tracker.update_from_mark_price(&mark));
        assert_eq!(tracker.atm_price().unwrap(), Price::from("50500.00"));
    }

    #[rstest]
    fn test_atm_tracker_mark_ignores_wrong_source_type() {
        let mut tracker = AtmTracker::new(AtmSource::IndexPrice(btc_perp()));
        let mark = MarkPriceUpdate {
            instrument_id: btc_perp(),
            value: Price::from("50500.00"),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };
        assert!(!tracker.update_from_mark_price(&mark));
        assert!(tracker.atm_price().is_none());
    }

    #[rstest]
    fn test_atm_tracker_update_from_index_price() {
        let mut tracker = AtmTracker::new(AtmSource::IndexPrice(btc_perp()));
        let index = IndexPriceUpdate {
            instrument_id: btc_perp(),
            value: Price::from("49900.00"),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };
        assert!(tracker.update_from_index_price(&index));
        assert_eq!(tracker.atm_price().unwrap(), Price::from("49900.00"));
    }

    #[rstest]
    fn test_atm_tracker_update_from_option_greeks() {
        use nautilus_model::data::option_chain::OptionGreeks;

        let mut tracker = AtmTracker::new(AtmSource::ForwardPrice);
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(50500.0),
            ..Default::default()
        };
        assert!(tracker.update_from_option_greeks(&greeks));
        assert_eq!(tracker.atm_price().unwrap(), Price::from("50500.00"));
    }

    #[rstest]
    fn test_atm_tracker_forward_ignores_none_underlying() {
        use nautilus_model::data::option_chain::OptionGreeks;

        let mut tracker = AtmTracker::new(AtmSource::ForwardPrice);
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: None,
            ..Default::default()
        };
        assert!(!tracker.update_from_option_greeks(&greeks));
        assert!(tracker.atm_price().is_none());
    }

    #[rstest]
    fn test_atm_tracker_non_forward_ignores_greeks() {
        use nautilus_model::data::option_chain::OptionGreeks;

        let mut tracker = AtmTracker::new(AtmSource::IndexPrice(btc_perp()));
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(50500.0),
            ..Default::default()
        };
        assert!(!tracker.update_from_option_greeks(&greeks));
        assert!(tracker.atm_price().is_none());
    }
}

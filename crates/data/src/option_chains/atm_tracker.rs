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
//!
//! ATM price is always derived from the exchange-provided forward price
//! embedded in each option greeks/ticker update.

use nautilus_model::{data::option_chain::OptionGreeks, types::Price};

/// Tracks the raw ATM price reactively from the forward price in option greeks.
///
/// Does not interact with cache — receives updates via handler callbacks.
/// Closest-strike resolution is delegated to `StrikeRange::resolve()`.
#[derive(Debug)]
pub struct AtmTracker {
    atm_price: Option<Price>,
    /// Precision used when converting forward prices from f64 to Price.
    forward_precision: u8,
}

impl AtmTracker {
    /// Creates a new [`AtmTracker`].
    pub fn new() -> Self {
        Self {
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

    /// Sets the initial ATM price (e.g. from a forward price fetched via HTTP).
    ///
    /// This allows instant bootstrap without waiting for the first WebSocket tick.
    /// Subsequent live updates will overwrite this value normally.
    pub fn set_initial_price(&mut self, price: Price) {
        self.atm_price = Some(price);
    }

    /// Updates from an option greeks event.
    ///
    /// Extracts `underlying_price` from the greeks — the exchange-provided
    /// forward price for this expiry. Returns `true` if the ATM price was updated.
    pub fn update_from_option_greeks(&mut self, greeks: &OptionGreeks) -> bool {
        if let Some(fwd) = greeks.underlying_price {
            self.atm_price = Some(Price::new(fwd, self.forward_precision));
            return true;
        }
        false
    }
}

impl Default for AtmTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::option_chain::OptionGreeks, identifiers::InstrumentId, types::Price,
    };
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_atm_tracker_initial_none() {
        let tracker = AtmTracker::new();
        assert!(tracker.atm_price().is_none());
    }

    #[rstest]
    fn test_atm_tracker_update_from_option_greeks() {
        let mut tracker = AtmTracker::new();
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
        let mut tracker = AtmTracker::new();
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: None,
            ..Default::default()
        };
        assert!(!tracker.update_from_option_greeks(&greeks));
        assert!(tracker.atm_price().is_none());
    }

    #[rstest]
    fn test_atm_tracker_set_initial_price() {
        let mut tracker = AtmTracker::new();
        tracker.set_initial_price(Price::from("50000.00"));
        assert_eq!(tracker.atm_price().unwrap(), Price::from("50000.00"));
    }

    #[rstest]
    fn test_atm_tracker_set_forward_precision() {
        let mut tracker = AtmTracker::new();
        tracker.set_forward_precision(4);
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(50500.1234),
            ..Default::default()
        };
        assert!(tracker.update_from_option_greeks(&greeks));
        assert_eq!(tracker.atm_price().unwrap(), Price::from("50500.1234"));
    }
}

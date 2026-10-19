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

//! Caches for accumulating Interactive Brokers tick updates.

use ahash::AHashMap;
use ibapi::contracts::{OptionComputation, tick_types::TickType};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{QuoteTick, greeks::OptionGreekValues, option_chain::OptionGreeks},
    enums::GreeksConvention,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

use super::{convert::IbPriceConverter, parse::is_representable_size};

/// IB publishes price -1 when a side has no quote; a zero or negative price
/// with genuine size is a legitimate value for spreads. The paired zero-size
/// sentinel is handled by the size updates, which clear the side.
// IB sends -1 without a size for a side that has no quote; -1 with a size is a real
// price, such as a spread's bid
pub(crate) fn is_sentinel_price(price: f64, size: Option<f64>) -> bool {
    !price.is_finite()
        || ((price + 1.0).abs() < f64::EPSILON && !size.is_some_and(|size| size > 0.0))
}

fn checked_quantity(size: f64, precision: u8) -> Option<Quantity> {
    let quantity = match Quantity::new_checked(size, precision) {
        Ok(quantity) => quantity,
        Err(e) => {
            tracing::warn!("Ignoring invalid IB quote size {size}: {e}");
            return None;
        }
    };

    if !is_representable_size(quantity, size, precision) {
        tracing::warn!(
            "Ignoring unrepresentable IB quote size {} with precision {}",
            size,
            precision
        );
        return None;
    }

    Some(quantity)
}

fn checked_price(price: anyhow::Result<Price>, ib_price: f64) -> Option<Price> {
    match price {
        Ok(price) => Some(price),
        Err(e) => {
            tracing::warn!("Ignoring invalid IB quote price {ib_price}: {e}");
            None
        }
    }
}

/// Quote cache that accumulates IB tick updates to build complete quotes.
///
/// Interactive Brokers sends individual tick price and size updates (bid price,
/// ask price, bid size, ask size). This cache accumulates these updates until
/// we have a complete quote with both bid and ask sides.
#[derive(Debug, Default)]
pub struct QuoteCache {
    /// Cached quote state per instrument.
    quotes: AHashMap<InstrumentId, CachedQuote>,
}

/// Cached quote state for an instrument.
#[derive(Debug, Clone)]
struct CachedQuote {
    /// Last bid price (tick type 1).
    bid_price: Option<f64>,
    /// Last ask price (tick type 2).
    ask_price: Option<f64>,
    /// Last bid size (tick type 0).
    bid_size: Option<f64>,
    /// Last ask size (tick type 3).
    ask_size: Option<f64>,
    /// Last emitted bid price (for filtering size-only updates).
    last_emitted_bid_price: Option<f64>,
    /// Last emitted ask price (for filtering size-only updates).
    last_emitted_ask_price: Option<f64>,
    /// Last complete quote tick (for fallback).
    last_complete_quote: Option<QuoteTick>,
}

impl QuoteCache {
    /// Create a new quote cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update bid price and return a complete quote if available.
    pub fn update_bid_price(
        &mut self,
        instrument_id: InstrumentId,
        price: f64,
        size: Option<f64>,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        if prices.is_sentinel(price, size) {
            if let Some(cached) = self.quotes.get_mut(&instrument_id) {
                cached.bid_price = None;
                cached.bid_size = None;
            }

            return None;
        }

        let cached = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| CachedQuote {
                bid_price: None,
                ask_price: None,
                bid_size: None,
                ask_size: None,
                last_emitted_bid_price: None,
                last_emitted_ask_price: None,
                last_complete_quote: None,
            });

        cached.bid_price = Some(price);
        self.try_build_quote(instrument_id, prices, size_precision, ts_event, ts_init)
    }

    /// Update ask price and return a complete quote if available.
    pub fn update_ask_price(
        &mut self,
        instrument_id: InstrumentId,
        price: f64,
        size: Option<f64>,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        if prices.is_sentinel(price, size) {
            if let Some(cached) = self.quotes.get_mut(&instrument_id) {
                cached.ask_price = None;
                cached.ask_size = None;
            }

            return None;
        }

        let cached = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| CachedQuote {
                bid_price: None,
                ask_price: None,
                bid_size: None,
                ask_size: None,
                last_emitted_bid_price: None,
                last_emitted_ask_price: None,
                last_complete_quote: None,
            });

        cached.ask_price = Some(price);
        self.try_build_quote(instrument_id, prices, size_precision, ts_event, ts_init)
    }

    /// Update bid size and return a complete quote if available.
    pub fn update_bid_size(
        &mut self,
        instrument_id: InstrumentId,
        size: f64,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        self.update_bid_size_with_filter(
            instrument_id,
            size,
            prices,
            size_precision,
            ts_event,
            ts_init,
            false,
        )
    }

    /// Update bid size and return a complete quote if available, with optional filtering.
    #[allow(clippy::too_many_arguments)]
    pub fn update_bid_size_with_filter(
        &mut self,
        instrument_id: InstrumentId,
        size: f64,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        ignore_size_only: bool,
    ) -> Option<QuoteTick> {
        if !size.is_finite() || size <= 0.0 {
            if let Some(cached) = self.quotes.get_mut(&instrument_id) {
                cached.bid_size = None;
            }

            return None;
        }

        checked_quantity(size, size_precision)?;

        let cached = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| CachedQuote {
                bid_price: None,
                ask_price: None,
                bid_size: None,
                ask_size: None,
                last_emitted_bid_price: None,
                last_emitted_ask_price: None,
                last_complete_quote: None,
            });

        // If filtering and we have emitted prices, check if this is a size-only update
        if ignore_size_only
            && let Some(last_bid) = cached.last_emitted_bid_price
            && let Some(current_bid) = cached.bid_price
        {
            // Prices are the same, this is a size-only update, skip it
            if (last_bid - current_bid).abs() < f64::EPSILON {
                cached.bid_size = Some(size);
                return None;
            }
        }

        cached.bid_size = Some(size);
        self.try_build_quote(instrument_id, prices, size_precision, ts_event, ts_init)
    }

    /// Update ask size and return a complete quote if available.
    pub fn update_ask_size(
        &mut self,
        instrument_id: InstrumentId,
        size: f64,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        self.update_ask_size_with_filter(
            instrument_id,
            size,
            prices,
            size_precision,
            ts_event,
            ts_init,
            false,
        )
    }

    /// Update ask size and return a complete quote if available, with optional filtering.
    #[allow(clippy::too_many_arguments)]
    pub fn update_ask_size_with_filter(
        &mut self,
        instrument_id: InstrumentId,
        size: f64,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        ignore_size_only: bool,
    ) -> Option<QuoteTick> {
        if !size.is_finite() || size <= 0.0 {
            if let Some(cached) = self.quotes.get_mut(&instrument_id) {
                cached.ask_size = None;
            }

            return None;
        }

        checked_quantity(size, size_precision)?;

        let cached = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| CachedQuote {
                bid_price: None,
                ask_price: None,
                bid_size: None,
                ask_size: None,
                last_emitted_bid_price: None,
                last_emitted_ask_price: None,
                last_complete_quote: None,
            });

        // If filtering and we have emitted prices, check if this is a size-only update
        if ignore_size_only
            && let Some(last_ask) = cached.last_emitted_ask_price
            && let Some(current_ask) = cached.ask_price
        {
            // Prices are the same, this is a size-only update, skip it
            if (last_ask - current_ask).abs() < f64::EPSILON {
                cached.ask_size = Some(size);
                return None;
            }
        }

        cached.ask_size = Some(size);
        self.try_build_quote(instrument_id, prices, size_precision, ts_event, ts_init)
    }

    /// Try to build a complete quote from cached data.
    fn try_build_quote(
        &mut self,
        instrument_id: InstrumentId,
        prices: IbPriceConverter,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        let cached = self.quotes.get_mut(&instrument_id)?;

        // Check if we have all required fields
        let bid_price = cached.bid_price?;
        let ask_price = cached.ask_price?;
        let bid_size = cached.bid_size?;
        let ask_size = cached.ask_size?;

        let bid_qty = checked_quantity(bid_size, size_precision)?;
        let ask_qty = checked_quantity(ask_size, size_precision)?;

        // Build the quote
        let quote = QuoteTick::new(
            instrument_id,
            checked_price(prices.bid_price(bid_price), bid_price)?,
            checked_price(prices.ask_price(ask_price), ask_price)?,
            bid_qty,
            ask_qty,
            ts_event,
            ts_init,
        );

        // Cache the complete quote
        cached.last_complete_quote = Some(quote);

        // Track emitted prices for filtering size-only updates
        cached.last_emitted_bid_price = Some(bid_price);
        cached.last_emitted_ask_price = Some(ask_price);

        Some(quote)
    }

    /// Clear all cached quotes.
    pub fn clear(&mut self) {
        self.quotes.clear();
    }

    /// Get the last complete quote for an instrument (if available).
    #[must_use]
    pub fn get_last_quote(&self, instrument_id: &InstrumentId) -> Option<&QuoteTick> {
        self.quotes
            .get(instrument_id)
            .and_then(|cached| cached.last_complete_quote.as_ref())
    }
}

/// Option greeks cache that merges IB option-computation and open-interest ticks.
#[derive(Debug, Default)]
pub struct OptionGreeksCache {
    greeks: AHashMap<InstrumentId, CachedOptionGreeks>,
}

#[derive(Debug, Clone, Default)]
struct CachedOptionGreeks {
    greeks: Option<OptionGreekValues>,
    mark_iv: Option<f64>,
    bid_iv: Option<f64>,
    ask_iv: Option<f64>,
    underlying_price: Option<f64>,
    open_interest: Option<f64>,
}

impl OptionGreeksCache {
    /// Create a new option greeks cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates cached state from an IB option computation tick.
    ///
    /// IB reports a magnified option's computation in the contract's currency, not in its market
    /// data unit: for an OZC option quoted in cents, the underlying price is in dollars and the
    /// greeks are per dollar. The computation is therefore already in the instrument's units and
    /// no price magnifier applies.
    pub fn update_from_computation(
        &mut self,
        instrument_id: InstrumentId,
        computation: &OptionComputation,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<OptionGreeks> {
        let cached = self.greeks.entry(instrument_id).or_default();

        match computation.field {
            TickType::ModelOption | TickType::DelayedModelOption => {
                let mut greeks = cached.greeks.unwrap_or_default();
                if let Some(delta) = computation.delta {
                    greeks.delta = delta;
                }

                if let Some(gamma) = computation.gamma {
                    greeks.gamma = gamma;
                }

                if let Some(vega) = computation.vega {
                    greeks.vega = vega;
                }

                if let Some(theta) = computation.theta {
                    greeks.theta = theta;
                }

                greeks.rho = 0.0; // IB does not publish rho in tickOptionComputation
                cached.greeks = Some(greeks);
                if let Some(mark_iv) = computation.implied_volatility {
                    cached.mark_iv = Some(mark_iv);
                }
            }
            TickType::BidOption | TickType::DelayedBidOption => {
                if let Some(bid_iv) = computation.implied_volatility {
                    cached.bid_iv = Some(bid_iv);
                }
            }
            TickType::AskOption | TickType::DelayedAskOption => {
                if let Some(ask_iv) = computation.implied_volatility {
                    cached.ask_iv = Some(ask_iv);
                }
            }
            TickType::LastOption
            | TickType::DelayedLastOption
            | TickType::CustOptionComputation => {}
            _ => return None,
        }

        if let Some(underlying_price) = computation.underlying_price {
            cached.underlying_price = Some(underlying_price);
        }

        self.try_build_greeks(instrument_id, ts_event, ts_init)
    }

    /// Updates cached state from an open-interest tick.
    pub fn update_open_interest(
        &mut self,
        instrument_id: InstrumentId,
        open_interest: f64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<OptionGreeks> {
        let cached = self.greeks.entry(instrument_id).or_default();
        cached.open_interest = Some(open_interest);
        self.try_build_greeks(instrument_id, ts_event, ts_init)
    }

    fn try_build_greeks(
        &self,
        instrument_id: InstrumentId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<OptionGreeks> {
        let cached = self.greeks.get(&instrument_id)?;
        let greeks = cached.greeks?;

        Some(OptionGreeks {
            instrument_id,
            greeks,
            convention: GreeksConvention::BlackScholes,
            mark_iv: cached.mark_iv,
            bid_iv: cached.bid_iv,
            ask_iv: cached.ask_iv,
            underlying_price: cached.underlying_price,
            open_interest: cached.open_interest,
            ts_event,
            ts_init,
        })
    }

    /// Clear all cached greeks.
    pub fn clear(&mut self) {
        self.greeks.clear();
    }
}

#[cfg(test)]
mod tests {
    use ibapi::contracts::{OptionComputation, tick_types::TickType};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::Instrument,
        types::Quantity,
    };
    use rstest::rstest;

    use super::{OptionGreeksCache, QuoteCache, is_sentinel_price};
    use crate::{data::convert::IbPriceConverter, stubs::currency_pair};

    fn instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("AAPL"), Venue::from("NASDAQ"))
    }

    #[rstest]
    #[case(-1.0, None, true)]
    #[case(-1.0, Some(0.0), true)]
    #[case(-1.0, Some(3.0), false)]
    #[case(f64::NAN, Some(3.0), true)]
    #[case(f64::INFINITY, None, true)]
    #[case(-1.25, None, false)]
    #[case(0.0, None, false)]
    #[case(99.5, None, false)]
    fn test_is_sentinel_price(
        #[case] price: f64,
        #[case] size: Option<f64>,
        #[case] expected: bool,
    ) {
        assert_eq!(is_sentinel_price(price, size), expected);
    }

    #[rstest]
    fn test_quote_cache_clears_side_on_magnified_sentinel() {
        // IB sends a magnified option's missing bid as -100 (OZC, magnifier 100, delayed data)
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();
        let prices = IbPriceConverter::new(5).with_price_magnifier(100);

        cache.update_bid_price(
            instrument_id,
            5.5,
            None,
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            5.875,
            None,
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_bid_size(
            instrument_id,
            10.0,
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        let quote = cache
            .update_ask_size(
                instrument_id,
                12.0,
                prices,
                0,
                UnixNanos::new(1),
                UnixNanos::new(1),
            )
            .unwrap();
        assert_eq!(quote.bid_price.as_f64(), 0.055);
        assert_eq!(quote.ask_price.as_f64(), 0.05875);

        let cleared = cache.update_bid_price(
            instrument_id,
            -100.0,
            None,
            prices,
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        let after = cache.update_ask_size(
            instrument_id,
            12.0,
            prices,
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );

        assert!(cleared.is_none());
        assert!(after.is_none());
    }

    #[rstest]
    fn test_quote_cache_emits_negative_spread_prices() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            -3.25,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            -2.75,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        cache.update_bid_size(
            instrument_id,
            5.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );
        let quote = cache
            .update_ask_size(
                instrument_id,
                7.0,
                IbPriceConverter::new(2),
                0,
                UnixNanos::new(4),
                UnixNanos::new(4),
            )
            .unwrap();

        assert_eq!(quote.bid_price.as_f64(), -3.25);
        assert_eq!(quote.ask_price.as_f64(), -2.75);
        assert_eq!(quote.bid_size.as_f64(), 5.0);
        assert_eq!(quote.ask_size.as_f64(), 7.0);
    }

    #[rstest]
    fn test_quote_cache_requires_both_prices() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        let quote = cache.update_bid_price(
            instrument_id,
            100.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );

        assert!(quote.is_none());
        assert!(cache.get_last_quote(&instrument_id).is_none());
    }

    #[rstest]
    fn test_quote_cache_requires_both_sizes() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        let quote = cache.update_ask_price(
            instrument_id,
            101.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );

        assert!(quote.is_none());
        cache.update_bid_size(
            instrument_id,
            10.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );
        let quote = cache
            .update_ask_size(
                instrument_id,
                11.0,
                IbPriceConverter::new(2),
                0,
                UnixNanos::new(4),
                UnixNanos::new(4),
            )
            .unwrap();
        assert_eq!(quote.bid_price.as_f64(), 100.0);
        assert_eq!(quote.ask_price.as_f64(), 101.0);
        assert_eq!(quote.bid_size.as_f64(), 10.0);
        assert_eq!(quote.ask_size.as_f64(), 11.0);
        assert!(cache.get_last_quote(&instrument_id).is_some());
    }

    #[rstest]
    fn test_quote_cache_filters_size_only_updates_when_enabled() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            101.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        cache.update_bid_size(
            instrument_id,
            10.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );
        cache.update_ask_size(
            instrument_id,
            11.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(4),
            UnixNanos::new(4),
        );

        let quote = cache.update_bid_size_with_filter(
            instrument_id,
            10.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(5),
            UnixNanos::new(5),
            true,
        );

        assert!(quote.is_none());
        let last_quote = cache.get_last_quote(&instrument_id).unwrap();
        assert_eq!(last_quote.bid_size.as_f64(), 10.0);
    }

    #[rstest]
    fn test_quote_cache_drops_unrepresentable_size() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            101.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        cache.update_bid_size(
            instrument_id,
            10.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );
        cache.update_ask_size(
            instrument_id,
            11.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(4),
            UnixNanos::new(4),
        );

        let quote = cache.update_bid_size(
            instrument_id,
            0.5,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(5),
            UnixNanos::new(5),
        );

        assert!(quote.is_none());
        let last_quote = cache.get_last_quote(&instrument_id).unwrap();
        assert_eq!(last_quote.bid_size.as_f64(), 10.0);
    }

    #[rstest]
    fn test_quote_cache_accepts_large_size_at_size_precision() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            1.16375,
            None,
            IbPriceConverter::new(5),
            2,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            1.1638,
            None,
            IbPriceConverter::new(5),
            2,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        cache.update_bid_size(
            instrument_id,
            2_500_000.0,
            IbPriceConverter::new(5),
            2,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );
        let quote = cache
            .update_ask_size(
                instrument_id,
                1_975_139.0,
                IbPriceConverter::new(5),
                2,
                UnixNanos::new(4),
                UnixNanos::new(4),
            )
            .unwrap();

        assert_eq!(quote.ask_size, Quantity::new(1_975_139.0, 2));
    }

    #[rstest]
    fn test_quote_cache_emits_update_after_price_change() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            101.0,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        cache.update_bid_size(
            instrument_id,
            10.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
        );
        cache.update_ask_size(
            instrument_id,
            11.0,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(4),
            UnixNanos::new(4),
        );

        let quote = cache.update_bid_price(
            instrument_id,
            100.5,
            None,
            IbPriceConverter::new(2),
            0,
            UnixNanos::new(5),
            UnixNanos::new(5),
        );

        assert!(quote.is_some());
        let quote = quote.unwrap();
        assert_eq!(quote.bid_price.as_f64(), 100.5);
        assert_eq!(quote.bid_size.as_f64(), 10.0);
    }

    #[rstest]
    fn test_option_greeks_cache_waits_for_model_tick_before_emitting() {
        let mut cache = OptionGreeksCache::new();
        let instrument_id = instrument_id();

        let bid_only = cache.update_from_computation(
            instrument_id,
            &OptionComputation {
                field: TickType::BidOption,
                implied_volatility: Some(0.24),
                underlying_price: Some(155.0),
                ..Default::default()
            },
            UnixNanos::new(1),
            UnixNanos::new(1),
        );

        assert!(bid_only.is_none());

        let model = cache.update_from_computation(
            instrument_id,
            &OptionComputation {
                field: TickType::ModelOption,
                implied_volatility: Some(0.25),
                delta: Some(0.55),
                gamma: Some(0.02),
                vega: Some(0.15),
                theta: Some(-0.05),
                underlying_price: Some(155.0),
                ..Default::default()
            },
            UnixNanos::new(2),
            UnixNanos::new(2),
        );

        let greeks = model.unwrap();
        assert_eq!(greeks.delta, 0.55);
        assert_eq!(greeks.gamma, 0.02);
        assert_eq!(greeks.vega, 0.15);
        assert_eq!(greeks.theta, -0.05);
        assert_eq!(greeks.rho, 0.0);
        assert_eq!(greeks.mark_iv, Some(0.25));
        assert_eq!(greeks.bid_iv, Some(0.24));
        assert_eq!(greeks.ask_iv, None);
        assert_eq!(greeks.underlying_price, Some(155.0));
        assert_eq!(greeks.open_interest, None);
    }

    #[rstest]
    fn test_option_greeks_cache_keeps_magnified_option_computation_units() {
        // OZCX6 C0480 (priceMagnifier 100) on the IB paper Gateway: quotes in cents (bid 5.5),
        // while the model computation is in dollars (option price 0.056, underlying 4.81)
        let mut cache = OptionGreeksCache::new();

        let computation = OptionComputation {
            field: TickType::DelayedModelOption,
            implied_volatility: Some(0.168_589_714_471_248_57),
            delta: Some(0.536_139_125_076_405),
            option_price: Some(0.055_970_557_886_452_07),
            gamma: Some(3.126_512_157_892_507_5),
            vega: Some(0.002_999_584_948_674_465),
            theta: Some(-0.002_886_804_069_145_994),
            underlying_price: Some(4.810_008_049_011_230_5),
            ..Default::default()
        };

        let greeks = cache
            .update_from_computation(
                instrument_id(),
                &computation,
                UnixNanos::new(1),
                UnixNanos::new(1),
            )
            .unwrap();

        assert_eq!(greeks.underlying_price, computation.underlying_price);
        assert_eq!(Some(greeks.delta), computation.delta);
        assert_eq!(Some(greeks.gamma), computation.gamma);
        assert_eq!(Some(greeks.vega), computation.vega);
        assert_eq!(Some(greeks.theta), computation.theta);
        assert_eq!(greeks.mark_iv, computation.implied_volatility);
    }

    #[rstest]
    fn test_option_greeks_cache_merges_open_interest_after_model_tick() {
        let mut cache = OptionGreeksCache::new();
        let instrument_id = instrument_id();

        let _ = cache.update_from_computation(
            instrument_id,
            &OptionComputation {
                field: TickType::ModelOption,
                implied_volatility: Some(0.25),
                delta: Some(0.55),
                gamma: Some(0.02),
                vega: Some(0.15),
                theta: Some(-0.05),
                underlying_price: Some(155.0),
                ..Default::default()
            },
            UnixNanos::new(1),
            UnixNanos::new(1),
        );

        let greeks = cache
            .update_open_interest(instrument_id, 1000.0, UnixNanos::new(2), UnixNanos::new(2))
            .unwrap();

        assert_eq!(greeks.open_interest, Some(1000.0));
        assert_eq!(greeks.mark_iv, Some(0.25));
        assert_eq!(greeks.delta, 0.55);
    }

    #[rstest]
    fn test_quote_cache_rounds_fx_quote_to_increment() {
        let mut cache = QuoteCache::new();
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);

        cache.update_bid_price(
            instrument.id(),
            1.12097,
            Some(1_000_000.0),
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument.id(),
            1.12098,
            Some(1_000_000.0),
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_bid_size(
            instrument.id(),
            1_000_000.0,
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        let quote = cache
            .update_ask_size(
                instrument.id(),
                1_000_000.0,
                prices,
                0,
                UnixNanos::new(1),
                UnixNanos::new(1),
            )
            .unwrap();

        assert_eq!(quote.bid_price.as_f64(), 1.12095);
        assert_eq!(quote.ask_price.as_f64(), 1.12100);
    }

    #[rstest]
    fn test_quote_cache_does_not_lock_tight_fx_quote() {
        let mut cache = QuoteCache::new();
        let instrument = currency_pair("EUR", "USD", 5, "0.00005");
        let prices = IbPriceConverter::for_instrument(&instrument, 1);
        let (ts_event, ts_init) = (UnixNanos::new(1), UnixNanos::new(1));

        cache.update_bid_price(
            instrument.id(),
            1.12098,
            Some(1_000_000.0),
            prices,
            0,
            ts_event,
            ts_init,
        );
        cache.update_ask_price(
            instrument.id(),
            1.12099,
            Some(1_000_000.0),
            prices,
            0,
            ts_event,
            ts_init,
        );
        cache.update_bid_size(instrument.id(), 1_000_000.0, prices, 0, ts_event, ts_init);
        let quote = cache
            .update_ask_size(instrument.id(), 1_000_000.0, prices, 0, ts_event, ts_init)
            .unwrap();

        assert_eq!(quote.bid_price.as_f64(), 1.12095);
        assert_eq!(quote.ask_price.as_f64(), 1.12100);
    }

    #[rstest]
    fn test_quote_cache_divides_magnified_quote() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();
        let prices = IbPriceConverter::new(4).with_price_magnifier(100);

        cache.update_bid_price(
            instrument_id,
            425.25,
            Some(10.0),
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            425.5,
            Some(20.0),
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_bid_size(
            instrument_id,
            10.0,
            prices,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        let quote = cache
            .update_ask_size(
                instrument_id,
                20.0,
                prices,
                0,
                UnixNanos::new(1),
                UnixNanos::new(1),
            )
            .unwrap();

        assert_eq!(quote.bid_price.as_f64(), 4.2525);
        assert_eq!(quote.ask_price.as_f64(), 4.255);
    }

    #[rstest]
    fn test_quote_cache_clears_side_on_sentinel_for_magnified_contract() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();
        let prices = IbPriceConverter::new(4).with_price_magnifier(100);
        let (ts_event, ts_init) = (UnixNanos::new(1), UnixNanos::new(1));

        cache.update_bid_price(
            instrument_id,
            425.25,
            Some(10.0),
            prices,
            0,
            ts_event,
            ts_init,
        );
        cache.update_ask_price(
            instrument_id,
            425.5,
            Some(20.0),
            prices,
            0,
            ts_event,
            ts_init,
        );
        cache.update_bid_size(instrument_id, 10.0, prices, 0, ts_event, ts_init);
        assert!(
            cache
                .update_ask_size(instrument_id, 20.0, prices, 0, ts_event, ts_init)
                .is_some()
        );

        // IB's no-quote sentinel arrives as the raw -1, before any conversion, and clears the
        // side, so a later size update has no bid to complete a quote with
        let cleared =
            cache.update_bid_price(instrument_id, -1.0, None, prices, 0, ts_event, ts_init);
        let quote = cache.update_ask_size(instrument_id, 30.0, prices, 0, ts_event, ts_init);

        assert!(cleared.is_none());
        assert!(quote.is_none());
    }
}

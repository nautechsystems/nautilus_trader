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
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
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
        self.try_build_quote(
            instrument_id,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        )
    }

    /// Update ask price and return a complete quote if available.
    pub fn update_ask_price(
        &mut self,
        instrument_id: InstrumentId,
        price: f64,
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
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
        self.try_build_quote(
            instrument_id,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        )
    }

    /// Update bid size and return a complete quote if available.
    pub fn update_bid_size(
        &mut self,
        instrument_id: InstrumentId,
        size: f64,
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        self.update_bid_size_with_filter(
            instrument_id,
            size,
            price_precision,
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
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        ignore_size_only: bool,
    ) -> Option<QuoteTick> {
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
        self.try_build_quote(
            instrument_id,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        )
    }

    /// Update ask size and return a complete quote if available.
    pub fn update_ask_size(
        &mut self,
        instrument_id: InstrumentId,
        size: f64,
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        self.update_ask_size_with_filter(
            instrument_id,
            size,
            price_precision,
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
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        ignore_size_only: bool,
    ) -> Option<QuoteTick> {
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
        self.try_build_quote(
            instrument_id,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        )
    }

    /// Try to build a complete quote from cached data.
    fn try_build_quote(
        &mut self,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<QuoteTick> {
        let cached = self.quotes.get_mut(&instrument_id)?;

        // Check if we have all required fields
        let bid_price = cached.bid_price?;
        let ask_price = cached.ask_price?;
        let bid_size = cached.bid_size.unwrap_or(0.0);
        let ask_size = cached.ask_size.unwrap_or(0.0);

        // Build the quote
        let quote = QuoteTick::new(
            instrument_id,
            Price::new(bid_price, price_precision),
            Price::new(ask_price, price_precision),
            Quantity::new(bid_size, size_precision),
            Quantity::new(ask_size, size_precision),
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
    use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};
    use rstest::rstest;

    use super::{OptionGreeksCache, QuoteCache};

    fn instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("AAPL"), Venue::from("NASDAQ"))
    }

    #[rstest]
    fn test_quote_cache_requires_both_prices() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        let quote = cache.update_bid_price(
            instrument_id,
            100.0,
            2,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );

        assert!(quote.is_none());
        assert!(cache.get_last_quote(&instrument_id).is_none());
    }

    #[rstest]
    fn test_quote_cache_builds_complete_quote_with_default_sizes() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            2,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        let quote = cache.update_ask_price(
            instrument_id,
            101.0,
            2,
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );

        assert!(quote.is_some());
        let quote = quote.unwrap();
        assert_eq!(quote.bid_price.as_f64(), 100.0);
        assert_eq!(quote.ask_price.as_f64(), 101.0);
        assert_eq!(quote.bid_size.as_f64(), 0.0);
        assert_eq!(quote.ask_size.as_f64(), 0.0);
        assert!(cache.get_last_quote(&instrument_id).is_some());
    }

    #[rstest]
    fn test_quote_cache_filters_size_only_updates_when_enabled() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            2,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            101.0,
            2,
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );

        let quote = cache.update_bid_size_with_filter(
            instrument_id,
            10.0,
            2,
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
            true,
        );

        assert!(quote.is_none());
        let last_quote = cache.get_last_quote(&instrument_id).unwrap();
        assert_eq!(last_quote.bid_size.as_f64(), 0.0);
    }

    #[rstest]
    fn test_quote_cache_emits_update_after_price_change() {
        let mut cache = QuoteCache::new();
        let instrument_id = instrument_id();

        cache.update_bid_price(
            instrument_id,
            100.0,
            2,
            0,
            UnixNanos::new(1),
            UnixNanos::new(1),
        );
        cache.update_ask_price(
            instrument_id,
            101.0,
            2,
            0,
            UnixNanos::new(2),
            UnixNanos::new(2),
        );
        cache.update_bid_size_with_filter(
            instrument_id,
            10.0,
            2,
            0,
            UnixNanos::new(3),
            UnixNanos::new(3),
            true,
        );

        let quote = cache.update_bid_price(
            instrument_id,
            100.5,
            2,
            0,
            UnixNanos::new(4),
            UnixNanos::new(4),
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
}

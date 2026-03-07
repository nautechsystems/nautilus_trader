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

//! Option chain data types for aggregated option series snapshots.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Display,
    ops::Deref,
};

use nautilus_core::UnixNanos;

use super::HasTsInit;
use crate::{
    data::{
        QuoteTick,
        greeks::{HasGreeks, OptionGreekValues},
    },
    identifiers::{InstrumentId, OptionSeriesId},
    types::Price,
};

/// Defines which strikes to include in an option chain subscription.
#[derive(Clone, Debug, PartialEq)]
pub enum StrikeRange {
    /// Subscribe to a fixed set of strike prices.
    Fixed(Vec<Price>),
    /// Subscribe to strikes relative to ATM: N strikes above and N below.
    AtmRelative {
        strikes_above: usize,
        strikes_below: usize,
    },
    /// Subscribe to strikes within a percentage band around ATM price.
    AtmPercent { pct: f64 },
}

impl StrikeRange {
    /// Resolves the filtered set of strikes from all available strikes.
    ///
    /// - `Fixed`: returns the fixed strikes directly (intersected with available).
    /// - `AtmRelative`: finds the closest strike to ATM, takes N above and N below.
    /// - `AtmPercent`: filters strikes within a percentage band around ATM.
    ///
    /// If `atm_price` is `None` for ATM-based variants, returns an empty vec
    /// (subscriptions are deferred until ATM is known).
    ///
    /// # Panics
    ///
    /// Panics if a strike price comparison returns `None` (i.e. a NaN price value).
    #[must_use]
    pub fn resolve(&self, atm_price: Option<Price>, all_strikes: &[Price]) -> Vec<Price> {
        match self {
            Self::Fixed(strikes) => {
                if all_strikes.is_empty() {
                    strikes.clone()
                } else {
                    let available: HashSet<Price> = all_strikes.iter().copied().collect();
                    strikes
                        .iter()
                        .filter(|s| available.contains(s))
                        .copied()
                        .collect()
                }
            }
            Self::AtmRelative {
                strikes_above,
                strikes_below,
            } => {
                let Some(atm) = atm_price else {
                    return vec![]; // Defer until ATM is known
                };
                // Find index of closest strike to ATM
                let atm_idx = match all_strikes
                    .binary_search_by(|s| s.as_f64().partial_cmp(&atm.as_f64()).unwrap())
                {
                    Ok(idx) => idx,
                    Err(idx) => {
                        if idx == 0 {
                            0
                        } else if idx >= all_strikes.len() {
                            all_strikes.len() - 1
                        } else {
                            // Pick the closer of the two neighbors
                            let diff_below = (all_strikes[idx - 1].as_f64() - atm.as_f64()).abs();
                            let diff_above = (all_strikes[idx].as_f64() - atm.as_f64()).abs();
                            if diff_below <= diff_above {
                                idx - 1
                            } else {
                                idx
                            }
                        }
                    }
                };
                let start = atm_idx.saturating_sub(*strikes_below);
                let end = (atm_idx + strikes_above + 1).min(all_strikes.len());
                all_strikes[start..end].to_vec()
            }
            Self::AtmPercent { pct } => {
                let Some(atm) = atm_price else {
                    return vec![]; // Defer until ATM is known
                };
                let atm_f = atm.as_f64();
                if atm_f == 0.0 {
                    return all_strikes.to_vec();
                }
                all_strikes
                    .iter()
                    .filter(|s| {
                        let pct_diff = ((s.as_f64() - atm_f) / atm_f).abs();
                        pct_diff <= *pct
                    })
                    .copied()
                    .collect()
            }
        }
    }
}

/// Exchange-provided option Greeks and implied volatility for a single instrument.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct OptionGreeks {
    /// The instrument ID these Greeks apply to.
    pub instrument_id: InstrumentId,
    /// Core Greek sensitivity values.
    pub greeks: OptionGreekValues,
    /// Mark implied volatility.
    pub mark_iv: Option<f64>,
    /// Bid implied volatility.
    pub bid_iv: Option<f64>,
    /// Ask implied volatility.
    pub ask_iv: Option<f64>,
    /// Underlying price at time of Greeks calculation.
    pub underlying_price: Option<f64>,
    /// Open interest for the instrument.
    pub open_interest: Option<f64>,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HasTsInit for OptionGreeks {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Deref for OptionGreeks {
    type Target = OptionGreekValues;
    fn deref(&self) -> &Self::Target {
        &self.greeks
    }
}

impl HasGreeks for OptionGreeks {
    fn greeks(&self) -> OptionGreekValues {
        self.greeks
    }
}

impl Default for OptionGreeks {
    fn default() -> Self {
        Self {
            instrument_id: InstrumentId::from("NULL.NULL"),
            greeks: OptionGreekValues::default(),
            mark_iv: None,
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Display for OptionGreeks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OptionGreeks({}, delta={:.4}, gamma={:.4}, vega={:.4}, theta={:.4}, mark_iv={:?})",
            self.instrument_id, self.delta, self.gamma, self.vega, self.theta, self.mark_iv
        )
    }
}

/// Combined quote and Greeks data for a single strike in an option chain.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct OptionStrikeData {
    /// The latest quote for this strike.
    pub quote: QuoteTick,
    /// Exchange-provided Greeks (if available).
    pub greeks: Option<OptionGreeks>,
}

/// A point-in-time snapshot of an option chain for a single series.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct OptionChainSlice {
    /// The option series identifier.
    pub series_id: OptionSeriesId,
    /// The current ATM strike price (if determined).
    pub atm_strike: Option<Price>,
    /// Call option data keyed by strike price (sorted).
    pub calls: BTreeMap<Price, OptionStrikeData>,
    /// Put option data keyed by strike price (sorted).
    pub puts: BTreeMap<Price, OptionStrikeData>,
    /// UNIX timestamp (nanoseconds) when the snapshot event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HasTsInit for OptionChainSlice {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Display for OptionChainSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OptionChainSlice({}, atm={:?}, calls={}, puts={})",
            self.series_id,
            self.atm_strike,
            self.calls.len(),
            self.puts.len()
        )
    }
}

impl OptionChainSlice {
    /// Creates a new empty [`OptionChainSlice`] for the given series.
    #[must_use]
    pub fn new(series_id: OptionSeriesId) -> Self {
        Self {
            series_id,
            atm_strike: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    /// Returns the number of call entries.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    /// Returns the number of put entries.
    #[must_use]
    pub fn put_count(&self) -> usize {
        self.puts.len()
    }

    /// Returns the call data for a given strike price.
    #[must_use]
    pub fn get_call(&self, strike: &Price) -> Option<&OptionStrikeData> {
        self.calls.get(strike)
    }

    /// Returns the put data for a given strike price.
    #[must_use]
    pub fn get_put(&self, strike: &Price) -> Option<&OptionStrikeData> {
        self.puts.get(strike)
    }

    /// Returns the call quote for a given strike price.
    #[must_use]
    pub fn get_call_quote(&self, strike: &Price) -> Option<&QuoteTick> {
        self.calls.get(strike).map(|d| &d.quote)
    }

    /// Returns the call Greeks for a given strike price.
    #[must_use]
    pub fn get_call_greeks(&self, strike: &Price) -> Option<&OptionGreeks> {
        self.calls.get(strike).and_then(|d| d.greeks.as_ref())
    }

    /// Returns the put quote for a given strike price.
    #[must_use]
    pub fn get_put_quote(&self, strike: &Price) -> Option<&QuoteTick> {
        self.puts.get(strike).map(|d| &d.quote)
    }

    /// Returns the put Greeks for a given strike price.
    #[must_use]
    pub fn get_put_greeks(&self, strike: &Price) -> Option<&OptionGreeks> {
        self.puts.get(strike).and_then(|d| d.greeks.as_ref())
    }

    /// Returns all strike prices present in the chain (union of calls and puts).
    #[must_use]
    pub fn strikes(&self) -> Vec<Price> {
        let mut strikes: Vec<Price> = self.calls.keys().chain(self.puts.keys()).copied().collect();
        strikes.sort();
        strikes.dedup();
        strikes
    }

    /// Returns the total number of unique strikes.
    #[must_use]
    pub fn strike_count(&self) -> usize {
        self.strikes().len()
    }

    /// Returns `true` if the chain has no data.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty() && self.puts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::{identifiers::Venue, types::Quantity};

    fn make_quote(instrument_id: InstrumentId) -> QuoteTick {
        QuoteTick::new(
            instrument_id,
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        )
    }

    fn make_series_id() -> OptionSeriesId {
        OptionSeriesId::new(
            Venue::new("DERIBIT"),
            ustr::Ustr::from("BTC"),
            ustr::Ustr::from("BTC"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        )
    }

    #[rstest]
    fn test_strike_range_fixed() {
        let range = StrikeRange::Fixed(vec![Price::from("50000"), Price::from("55000")]);
        assert_eq!(
            range,
            StrikeRange::Fixed(vec![Price::from("50000"), Price::from("55000")])
        );
    }

    #[rstest]
    fn test_strike_range_atm_relative() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 5,
            strikes_below: 5,
        };

        if let StrikeRange::AtmRelative {
            strikes_above,
            strikes_below,
        } = range
        {
            assert_eq!(strikes_above, 5);
            assert_eq!(strikes_below, 5);
        } else {
            panic!("Expected AtmRelative variant");
        }
    }

    #[rstest]
    fn test_strike_range_atm_percent() {
        let range = StrikeRange::AtmPercent { pct: 0.1 };
        if let StrikeRange::AtmPercent { pct } = range {
            assert!((pct - 0.1).abs() < f64::EPSILON);
        } else {
            panic!("Expected AtmPercent variant");
        }
    }

    #[rstest]
    fn test_option_greeks_default_fields() {
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            greeks: OptionGreekValues::default(),
            mark_iv: None,
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        assert_eq!(greeks.delta, 0.0);
        assert_eq!(greeks.gamma, 0.0);
        assert_eq!(greeks.vega, 0.0);
        assert_eq!(greeks.theta, 0.0);
        assert!(greeks.mark_iv.is_none());
    }

    #[rstest]
    fn test_option_greeks_display() {
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            greeks: OptionGreekValues {
                delta: 0.55,
                gamma: 0.001,
                vega: 10.0,
                theta: -5.0,
                rho: 0.0,
            },
            mark_iv: Some(0.65),
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        let display = format!("{greeks}");
        assert!(display.contains("OptionGreeks"));
        assert!(display.contains("0.55"));
    }

    #[rstest]
    fn test_option_chain_slice_empty() {
        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };

        assert!(slice.is_empty());
        assert_eq!(slice.strike_count(), 0);
        assert!(slice.strikes().is_empty());
    }

    #[rstest]
    fn test_option_chain_slice_with_data() {
        let call_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let put_id = InstrumentId::from("BTC-20240101-50000-P.DERIBIT");
        let strike = Price::from("50000");

        let mut calls = BTreeMap::new();
        calls.insert(
            strike,
            OptionStrikeData {
                quote: make_quote(call_id),
                greeks: Some(OptionGreeks {
                    instrument_id: call_id,
                    greeks: OptionGreekValues {
                        delta: 0.55,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            },
        );

        let mut puts = BTreeMap::new();
        puts.insert(
            strike,
            OptionStrikeData {
                quote: make_quote(put_id),
                greeks: None,
            },
        );

        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: Some(strike),
            calls,
            puts,
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };

        assert!(!slice.is_empty());
        assert_eq!(slice.strike_count(), 1);
        assert_eq!(slice.strikes(), vec![strike]);
        assert!(slice.get_call(&strike).is_some());
        assert!(slice.get_put(&strike).is_some());
        assert!(slice.get_call_greeks(&strike).is_some());
        assert!(slice.get_put_greeks(&strike).is_none());
        assert_eq!(slice.get_call_greeks(&strike).unwrap().delta, 0.55);
    }

    #[rstest]
    fn test_option_chain_slice_display() {
        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };

        let display = format!("{slice}");
        assert!(display.contains("OptionChainSlice"));
        assert!(display.contains("DERIBIT"));
    }

    #[rstest]
    fn test_option_chain_slice_ts_init() {
        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(42u64),
        };

        assert_eq!(slice.ts_init(), UnixNanos::from(42u64));
    }

    // -- StrikeRange::resolve tests --

    #[rstest]
    fn test_strike_range_resolve_fixed() {
        let range = StrikeRange::Fixed(vec![Price::from("50000"), Price::from("55000")]);
        let result = range.resolve(None, &[]);
        assert_eq!(result, vec![Price::from("50000"), Price::from("55000")]);
    }

    #[rstest]
    fn test_strike_range_resolve_atm_relative() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 2,
            strikes_below: 2,
        };
        let strikes: Vec<Price> = [45000, 47000, 50000, 53000, 55000, 57000]
            .iter()
            .map(|s| Price::from(&s.to_string()))
            .collect();
        let atm = Some(Price::from("50000"));
        let result = range.resolve(atm, &strikes);
        // ATM at index 2, below=2 → start=0, above=2 → end=5
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], Price::from("45000"));
        assert_eq!(result[4], Price::from("55000"));
    }

    #[rstest]
    fn test_strike_range_resolve_atm_relative_no_atm() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 2,
            strikes_below: 2,
        };
        let strikes = vec![Price::from("50000"), Price::from("55000")];
        let result = range.resolve(None, &strikes);
        // No ATM → return empty (deferred until ATM known)
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_strike_range_resolve_atm_percent() {
        let range = StrikeRange::AtmPercent { pct: 0.1 }; // 10%
        let strikes: Vec<Price> = [45000, 48000, 50000, 52000, 55000, 60000]
            .iter()
            .map(|s| Price::from(&s.to_string()))
            .collect();
        let atm = Some(Price::from("50000"));
        let result = range.resolve(atm, &strikes);
        // 10% of 50000 = 5000, so [45000..55000] inclusive (<=)
        assert_eq!(result.len(), 5); // 45000, 48000, 50000, 52000, 55000
        assert!(result.contains(&Price::from("45000")));
        assert!(result.contains(&Price::from("48000")));
        assert!(result.contains(&Price::from("50000")));
        assert!(result.contains(&Price::from("52000")));
        assert!(result.contains(&Price::from("55000")));
    }

    #[rstest]
    fn test_option_chain_slice_new_empty() {
        let slice = OptionChainSlice::new(make_series_id());
        assert!(slice.is_empty());
        assert_eq!(slice.call_count(), 0);
        assert_eq!(slice.put_count(), 0);
        assert!(slice.atm_strike.is_none());
    }
}

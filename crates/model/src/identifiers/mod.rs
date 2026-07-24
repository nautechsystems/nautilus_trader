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

//! Identifiers for the trading domain model.
//!
//! # Design notes
//!
//! - `TradeId` remains a fixed-size `StackStr` with a 36-character limit.
//! - High-cardinality external IDs must not use `Ustr`, because interning
//!   unique values grows process memory without bound.
//! - Some identifiers still use fixed-size `repr(C)` storage because the
//!   current Cython/C ABI shares raw layout by value.
//! - A deeper storage redesign is deferred to V2, when the ABI can move to
//!   conversion-based bindings instead of layout sharing.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[macro_use]
mod macros;

pub mod account_id;
pub mod actor_id;
pub mod client_id;
pub mod client_order_id;
pub mod component_id;
pub mod exec_algorithm_id;
pub mod instrument_id;
pub mod option_series_id;
pub mod order_list_id;
pub mod position_id;
pub mod strategy_id;
pub mod symbol;
pub mod trade_id;
pub mod trader_id;
pub mod venue;
pub mod venue_order_id;

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

// Re-exports
pub use crate::identifiers::{
    account_id::AccountId,
    actor_id::ActorId,
    client_id::ClientId,
    client_order_id::ClientOrderId,
    component_id::ComponentId,
    exec_algorithm_id::ExecAlgorithmId,
    instrument_id::{GENERIC_SPREAD_ID_SEPARATOR, InstrumentId, InstrumentIdError},
    option_series_id::{OptionSeriesId, OptionSeriesIdError},
    order_list_id::OrderListId,
    position_id::PositionId,
    strategy_id::{StrategyId, normalize_order_id_tag},
    symbol::Symbol,
    trade_id::TradeId,
    trader_id::TraderId,
    venue::Venue,
    venue_order_id::VenueOrderId,
};

/// Creates a generic spread instrument ID from `(instrument_id, ratio)` legs.
///
/// Matches Python `new_generic_spread_id`: validates non-zero ratios and a
/// common venue, sorts legs alphabetically by symbol, and formats negative
/// ratios as `((ratio))symbol`.
///
/// # Errors
///
/// Returns an error when there are fewer than two legs, a ratio is zero, venues
/// differ, or a ratio cannot be represented as an absolute value.
pub fn new_generic_spread_id(
    instrument_ratios: &[(InstrumentId, i64)],
) -> anyhow::Result<InstrumentId> {
    anyhow::ensure!(
        instrument_ratios.len() > 1,
        "instrument_ratios list needs to have at least 2 legs"
    );

    let first_venue = instrument_ratios[0].0.venue;
    for (instrument_id, ratio) in instrument_ratios {
        anyhow::ensure!(*ratio != 0, "ratio cannot be zero");
        anyhow::ensure!(
            instrument_id.venue == first_venue,
            "All venues must match. Expected {}, was {}",
            first_venue,
            instrument_id.venue
        );
    }

    let mut sorted_ratios = instrument_ratios.to_vec();
    sorted_ratios.sort_by_key(|(instrument_id, _)| instrument_id.symbol.to_string());

    let symbol_parts = sorted_ratios
        .into_iter()
        .map(|(instrument_id, ratio)| {
            if ratio > 0 {
                Ok(format!("({ratio}){}", instrument_id.symbol))
            } else {
                let abs_ratio = ratio
                    .checked_abs()
                    .ok_or_else(|| anyhow::anyhow!("ratio cannot be i64::MIN"))?;
                Ok(format!("(({abs_ratio})){}", instrument_id.symbol))
            }
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(InstrumentId::new(
        Symbol::new(symbol_parts.join(GENERIC_SPREAD_ID_SEPARATOR)),
        first_venue,
    ))
}

/// Parses a generic spread instrument ID into `(instrument_id, ratio)` legs.
///
/// Returns `None` when the symbol is not in the generic spread format produced by
/// Python `new_generic_spread_id`.
#[must_use]
pub fn parse_generic_spread_id_legs(
    instrument_id: &InstrumentId,
) -> Option<Vec<(InstrumentId, i64)>> {
    let symbol = instrument_id.symbol.as_str();
    if !symbol.contains(GENERIC_SPREAD_ID_SEPARATOR) {
        return None;
    }

    symbol
        .split(GENERIC_SPREAD_ID_SEPARATOR)
        .map(|component| parse_generic_spread_leg(component, instrument_id.venue))
        .collect()
}

fn parse_generic_spread_leg(component: &str, venue: Venue) -> Option<(InstrumentId, i64)> {
    if let Some(rest) = component.strip_prefix("((") {
        let (ratio, symbol) = rest.split_once("))")?;
        return parse_generic_spread_leg_parts(ratio, symbol, venue, -1);
    }

    let rest = component.strip_prefix('(')?;
    let (ratio, symbol) = rest.split_once(')')?;
    parse_generic_spread_leg_parts(ratio, symbol, venue, 1)
}

fn parse_generic_spread_leg_parts(
    ratio: &str,
    symbol: &str,
    venue: Venue,
    sign: i64,
) -> Option<(InstrumentId, i64)> {
    if symbol.is_empty() {
        return None;
    }

    // Only digit ratios are valid: `parse::<i64>` would accept a leading sign,
    // silently flipping the sign encoded by the surrounding parentheses.
    if ratio.is_empty() || !ratio.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let ratio = ratio.parse::<i64>().ok()?.checked_mul(sign)?;
    if ratio == 0 {
        return None;
    }

    Some((InstrumentId::new(Symbol::new(symbol), venue), ratio))
}

impl_from_str_for_identifier!(account_id::AccountId);
impl_from_str_for_identifier!(actor_id::ActorId);
impl_from_str_for_identifier!(client_id::ClientId);
impl_from_str_for_identifier!(client_order_id::ClientOrderId);
impl_from_str_for_identifier!(component_id::ComponentId);
impl_from_str_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_from_str_for_identifier!(order_list_id::OrderListId);
impl_from_str_for_identifier!(position_id::PositionId);
impl_from_str_for_identifier!(strategy_id::StrategyId);
impl_from_str_for_identifier!(symbol::Symbol);
impl_from_str_for_identifier!(trade_id::TradeId);
impl_from_str_for_identifier!(trader_id::TraderId);
impl_from_str_for_identifier!(venue::Venue);
impl_from_str_for_identifier!(venue_order_id::VenueOrderId);

impl_serialization_for_identifier!(account_id::AccountId);
impl_serialization_for_identifier!(actor_id::ActorId);
impl_serialization_for_identifier!(client_id::ClientId);
impl_serialization_for_identifier!(client_order_id::ClientOrderId);
impl_serialization_for_identifier!(component_id::ComponentId);
impl_serialization_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_serialization_for_identifier!(order_list_id::OrderListId);
impl_serialization_for_identifier!(position_id::PositionId);
impl_serialization_for_identifier!(strategy_id::StrategyId);
impl_serialization_for_identifier!(symbol::Symbol);
impl_serialization_for_identifier!(trader_id::TraderId);
impl_serialization_for_identifier!(venue::Venue);
impl_serialization_for_identifier!(venue_order_id::VenueOrderId);

impl_as_ref_for_identifier!(account_id::AccountId);
impl_as_ref_for_identifier!(actor_id::ActorId);
impl_as_ref_for_identifier!(client_id::ClientId);
impl_as_ref_for_identifier!(client_order_id::ClientOrderId);
impl_as_ref_for_identifier!(component_id::ComponentId);
impl_as_ref_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_as_ref_for_identifier!(order_list_id::OrderListId);
impl_as_ref_for_identifier!(position_id::PositionId);
impl_as_ref_for_identifier!(strategy_id::StrategyId);
impl_as_ref_for_identifier!(symbol::Symbol);
impl_as_ref_for_identifier!(trader_id::TraderId);
impl_as_ref_for_identifier!(venue::Venue);
impl_as_ref_for_identifier!(venue_order_id::VenueOrderId);

/// Print interned string cache statistics for debugging purposes.
pub fn interned_string_stats() {
    ustr::total_allocated();
    ustr::total_capacity();

    ustr::string_cache_iter().for_each(|s| println!("{s}"));
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{InstrumentId, new_generic_spread_id, parse_generic_spread_id_legs};

    #[rstest]
    fn test_new_generic_spread_id_sorts_and_formats_legs() {
        let msft = InstrumentId::from("MSFT.NASDAQ");
        let aapl = InstrumentId::from("AAPL.NASDAQ");

        let spread = new_generic_spread_id(&[(msft, 1), (aapl, -2)]).unwrap();

        assert_eq!(spread, InstrumentId::from("((2))AAPL___(1)MSFT.NASDAQ"));
    }

    #[rstest]
    fn test_new_generic_spread_id_rejects_invalid_legs() {
        assert!(new_generic_spread_id(&[(InstrumentId::from("MSFT.NASDAQ"), 1)]).is_err());
        assert!(
            new_generic_spread_id(&[
                (InstrumentId::from("MSFT.NASDAQ"), 0),
                (InstrumentId::from("AAPL.NASDAQ"), 1),
            ])
            .is_err()
        );
        assert!(
            new_generic_spread_id(&[
                (InstrumentId::from("MSFT.NASDAQ"), 1),
                (InstrumentId::from("AAPL.XNAS"), 1),
            ])
            .is_err()
        );
    }

    #[rstest]
    #[case("((-2))AAPL___(1)MSFT.NASDAQ")] // signed ratio in negative leg
    #[case("(+1)MSFT___(2)AAPL.NASDAQ")] // signed ratio in positive leg
    #[case("(-1)MSFT___(2)AAPL.NASDAQ")] // signed ratio in positive leg
    #[case("()MSFT___(2)AAPL.NASDAQ")] // empty ratio
    #[case("(1a)MSFT___(2)AAPL.NASDAQ")] // non-digit ratio
    fn test_parse_generic_spread_id_legs_rejects_non_digit_ratios(#[case] value: &str) {
        assert!(parse_generic_spread_id_legs(&InstrumentId::from(value)).is_none());
    }

    #[rstest]
    fn test_generic_spread_id_round_trip() {
        let spread = new_generic_spread_id(&[
            (InstrumentId::from("ESM4 P5230.XCME"), -1),
            (InstrumentId::from("ESM4 P5250.XCME"), 1),
        ])
        .unwrap();

        assert_eq!(
            parse_generic_spread_id_legs(&spread).unwrap(),
            vec![
                (InstrumentId::from("ESM4 P5230.XCME"), -1),
                (InstrumentId::from("ESM4 P5250.XCME"), 1),
            ]
        );
    }
}

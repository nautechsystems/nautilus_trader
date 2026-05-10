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

//! Symbol, instrument and decimal conversion utilities.

use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::common::{consts::BULLET_VENUE, error::BulletError};

// ── Symbol / InstrumentId ──────────────────────────────────────────────────────

/// Convert a Bullet native symbol (e.g. `"BTC-USD"`) to a Nautilus `InstrumentId`.
///
/// Convention: `<BASE>-<QUOTE>-PERP.BULLET`
#[must_use]
pub fn instrument_id_from_symbol(symbol: &str) -> InstrumentId {
    let nautilus_symbol = format!("{symbol}-PERP");
    InstrumentId::new(
        Symbol::new(Ustr::from(&nautilus_symbol)),
        *BULLET_VENUE,
    )
}

/// Extract the Bullet native symbol from a Nautilus `InstrumentId`.
///
/// Strips the `-PERP` suffix and the `.BULLET` venue.
///
/// # Errors
///
/// Returns an error if the symbol does not have the expected `-PERP.BULLET` suffix.
pub fn symbol_from_instrument_id(instrument_id: &InstrumentId) -> Result<String, BulletError> {
    let symbol_str = instrument_id.symbol.as_str();
    symbol_str
        .strip_suffix("-PERP")
        .map(str::to_string)
        .ok_or_else(|| {
            BulletError::Parse(format!(
                "instrument id '{instrument_id}' does not have expected -PERP suffix"
            ))
        })
}

/// Return the Bullet venue.
#[must_use]
pub fn bullet_venue() -> &'static Venue {
    &BULLET_VENUE
}

// ── Decimal helpers ────────────────────────────────────────────────────────────

/// Parse a decimal string, returning an error on failure.
///
/// # Errors
///
/// Returns a `BulletError::Parse` if the string is not a valid decimal.
pub fn parse_decimal(s: &str) -> Result<Decimal, BulletError> {
    s.parse()
        .map_err(|e| BulletError::Parse(format!("cannot parse decimal '{s}': {e}")))
}

/// Parse an optional decimal string.
///
/// Returns `None` for empty or absent strings; errors on malformed input.
///
/// # Errors
///
/// Returns a `BulletError::Parse` if the string is non-empty but not a valid decimal.
pub fn parse_optional_decimal(s: Option<&str>) -> Result<Option<Decimal>, BulletError> {
    match s {
        None | Some("") => Ok(None),
        Some(v) => Ok(Some(parse_decimal(v)?)),
    }
}

// ── Price / quantity snapping ──────────────────────────────────────────────────
//
// Logic ported from bullet-bots-personal/crates/exchanges/bullet/src/broker.rs:65-71.
// Bullet's matching engine rejects prices/quantities that are not aligned to tick_size/step_size.

/// Round a price *down* to the nearest tick (for buy orders) or *up* (for sell orders).
///
/// Returns `price` unchanged when `tick_size` is zero or None.
#[must_use]
pub fn snap_price(price: Decimal, tick_size: Option<Decimal>, is_buy: bool) -> Decimal {
    let Some(tick) = tick_size.filter(|t| !t.is_zero()) else {
        return price;
    };
    if is_buy {
        round_down_to_tick(price, tick)
    } else {
        round_up_to_tick(price, tick)
    }
}

/// Round `qty` *down* to the nearest step size.
///
/// Returns `qty` unchanged when `step_size` is zero or None.
#[must_use]
pub fn snap_qty(qty: Decimal, step_size: Option<Decimal>) -> Decimal {
    let Some(step) = step_size.filter(|s| !s.is_zero()) else {
        return qty;
    };
    round_down_to_step(qty, step)
}

/// Round a value down to the nearest multiple of `tick`.
#[must_use]
pub fn round_down_to_tick(value: Decimal, tick: Decimal) -> Decimal {
    (value / tick).floor() * tick
}

/// Round a value up to the nearest multiple of `tick`.
#[must_use]
pub fn round_up_to_tick(value: Decimal, tick: Decimal) -> Decimal {
    (value / tick).ceil() * tick
}

/// Round a value down to the nearest multiple of `step`.
#[must_use]
pub fn round_down_to_step(value: Decimal, step: Decimal) -> Decimal {
    (value / step).floor() * step
}

// ── Order levels parsing ───────────────────────────────────────────────────────

/// Parse a sequence of `[price, qty]` string pairs into `Vec<(Decimal, Decimal)>`.
///
/// # Errors
///
/// Returns an error if any price or quantity string is not a valid decimal.
pub fn parse_order_levels(
    levels: &[[String; 2]],
) -> Result<Vec<(Decimal, Decimal)>, BulletError> {
    levels
        .iter()
        .map(|[price, qty]| Ok((parse_decimal(price)?, parse_decimal(qty)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_instrument_id_from_symbol() {
        let id = instrument_id_from_symbol("BTC-USD");
        assert_eq!(id.symbol.as_str(), "BTC-USD-PERP");
        assert_eq!(id.venue.as_str(), "BULLET");
    }

    #[rstest]
    fn test_symbol_from_instrument_id() {
        let id = instrument_id_from_symbol("ETH-USD");
        let sym = symbol_from_instrument_id(&id).unwrap();
        assert_eq!(sym, "ETH-USD");
    }

    #[rstest]
    fn test_snap_price_buy_rounds_down() {
        let price = dec!(50001.3);
        let tick = dec!(0.1);
        let snapped = snap_price(price, Some(tick), true);
        assert_eq!(snapped, dec!(50001.3));
    }

    #[rstest]
    fn test_snap_price_sell_rounds_up() {
        let price = dec!(50001.21);
        let tick = dec!(0.5);
        let snapped = snap_price(price, Some(tick), false);
        assert_eq!(snapped, dec!(50001.5));
    }

    #[rstest]
    fn test_snap_qty_rounds_down() {
        let qty = dec!(0.123456);
        let step = dec!(0.001);
        let snapped = snap_qty(qty, Some(step));
        assert_eq!(snapped, dec!(0.123));
    }

    #[rstest]
    fn test_parse_order_levels() {
        let levels = vec![
            ["50000.5".to_string(), "1.5".to_string()],
            ["49999.0".to_string(), "2.0".to_string()],
        ];
        let parsed = parse_order_levels(&levels).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, dec!(50000.5));
        assert_eq!(parsed[0].1, dec!(1.5));
    }
}

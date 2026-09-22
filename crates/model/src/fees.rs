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

//! Account-owned fee schedules decoupled from instrument definitions.
//!
//! Fee policy belongs to the execution runtime serving an account, not to the
//! instrument. Two accounts can trade the same instrument with different fees
//! without constructing different instrument definitions.

use ahash::AHashMap;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    enums::LiquiditySide,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::{Money, Price, Quantity},
};

/// Explicit maker/taker fee rates as a fraction of notional value.
///
/// Rates are expressed as decimals (for example `0.001` is 0.1%). Negative
/// maker rates represent rebates where the venue supports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MakerTakerFeeRates {
    /// The maker fee rate.
    pub maker: Decimal,
    /// The taker fee rate.
    pub taker: Decimal,
}

impl MakerTakerFeeRates {
    /// Creates new [`MakerTakerFeeRates`] with explicit rates.
    #[must_use]
    pub const fn new(maker: Decimal, taker: Decimal) -> Self {
        Self { maker, taker }
    }

    /// Returns an explicit zero-fee rate pair.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            maker: Decimal::ZERO,
            taker: Decimal::ZERO,
        }
    }

    /// Returns the rate for the given liquidity side.
    ///
    /// # Errors
    ///
    /// Returns an error if `liquidity_side` is [`LiquiditySide::NoLiquiditySide`].
    pub fn rate_for(&self, liquidity_side: LiquiditySide) -> anyhow::Result<Decimal> {
        match liquidity_side {
            LiquiditySide::Maker => Ok(self.maker),
            LiquiditySide::Taker => Ok(self.taker),
            LiquiditySide::NoLiquiditySide => {
                anyhow::bail!("Invalid `LiquiditySide`: {liquidity_side}")
            }
        }
    }
}

/// Account-owned maker/taker fee schedule with exact instrument overrides.
///
/// Resolution is deterministic: an exact [`InstrumentId`] override wins, otherwise
/// the default applies. Absent configuration is represented by the absence of a
/// schedule (`None` at the owner), which is distinct from an explicit zero rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MakerTakerFeeSchedule {
    /// The default rates applied when no override matches.
    pub default: MakerTakerFeeRates,
    /// Exact per-instrument rate overrides.
    pub overrides: AHashMap<InstrumentId, MakerTakerFeeRates>,
}

impl MakerTakerFeeSchedule {
    /// Creates a new schedule with explicit default rates and no overrides.
    #[must_use]
    pub fn new(maker: Decimal, taker: Decimal) -> Self {
        Self {
            default: MakerTakerFeeRates::new(maker, taker),
            overrides: AHashMap::new(),
        }
    }

    /// Creates a new explicit zero-fee schedule.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            default: MakerTakerFeeRates::zero(),
            overrides: AHashMap::new(),
        }
    }

    /// Adds or replaces an exact instrument override.
    pub fn set_override(&mut self, instrument_id: InstrumentId, rates: MakerTakerFeeRates) {
        self.overrides.insert(instrument_id, rates);
    }

    /// Returns the resolved rates for the given instrument.
    #[must_use]
    pub fn rates_for(&self, instrument_id: InstrumentId) -> MakerTakerFeeRates {
        self.overrides
            .get(&instrument_id)
            .copied()
            .unwrap_or(self.default)
    }

    /// Returns the resolved rate for the given instrument and liquidity side.
    ///
    /// # Errors
    ///
    /// Returns an error if `liquidity_side` is [`LiquiditySide::NoLiquiditySide`].
    pub fn rate_for(
        &self,
        instrument_id: InstrumentId,
        liquidity_side: LiquiditySide,
    ) -> anyhow::Result<Decimal> {
        self.rates_for(instrument_id).rate_for(liquidity_side)
    }
}

/// Calculates maker/taker commission from an explicitly resolved fee rate.
///
/// This is the single shared arithmetic for notional-based maker/taker fees used
/// by both account calculations and execution fee models. Contract terms (notional
/// and currency) come from the instrument; fee policy (the rate) comes from the
/// account-owned schedule and must already be resolved by the caller.
///
/// # Errors
///
/// Returns an error if the notional value cannot be calculated, arithmetic
/// overflows, or the commission cannot be represented in the notional currency.
pub fn calculate_maker_taker_commission(
    instrument: &InstrumentAny,
    last_qty: Quantity,
    last_px: Price,
    fee_rate: Decimal,
    use_quote_for_inverse: Option<bool>,
) -> anyhow::Result<Money> {
    let notional =
        instrument.try_calculate_notional_value(last_qty, last_px, use_quote_for_inverse)?;
    let commission = notional
        .as_decimal()
        .checked_mul(fee_rate)
        .ok_or_else(|| anyhow::anyhow!("commission calculation overflow"))?;
    Money::from_decimal(commission, notional.currency).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::enums::LiquiditySide;

    #[rstest]
    fn test_rates_for_liquidity_side() {
        let rates = MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0005));
        assert_eq!(rates.rate_for(LiquiditySide::Maker).unwrap(), dec!(0.0002));
        assert_eq!(rates.rate_for(LiquiditySide::Taker).unwrap(), dec!(0.0005));
        assert!(rates.rate_for(LiquiditySide::NoLiquiditySide).is_err());
    }

    #[rstest]
    fn test_schedule_override_wins_over_default() {
        let mut schedule = MakerTakerFeeSchedule::new(dec!(0.001), dec!(0.002));
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let other_id = InstrumentId::from("ETHUSDT.BINANCE");
        schedule.set_override(
            instrument_id,
            MakerTakerFeeRates::new(dec!(0.0001), dec!(0.0002)),
        );

        assert_eq!(
            schedule.rates_for(instrument_id),
            MakerTakerFeeRates::new(dec!(0.0001), dec!(0.0002))
        );
        assert_eq!(
            schedule.rates_for(other_id),
            MakerTakerFeeRates::new(dec!(0.001), dec!(0.002))
        );
    }

    #[rstest]
    fn test_explicit_zero_distinguishable_from_default() {
        let mut schedule = MakerTakerFeeSchedule::new(dec!(0.001), dec!(0.002));
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        schedule.set_override(instrument_id, MakerTakerFeeRates::zero());

        assert_eq!(
            schedule
                .rate_for(instrument_id, LiquiditySide::Taker)
                .unwrap(),
            Decimal::ZERO
        );
        assert_eq!(
            schedule
                .rate_for(InstrumentId::from("ETHUSDT.BINANCE"), LiquiditySide::Taker)
                .unwrap(),
            dec!(0.002)
        );
    }

    #[rstest]
    fn test_commission_matches_notional_arithmetic() {
        use crate::{
            instruments::{Instrument, stubs::audusd_sim},
            types::{Price, Quantity},
        };

        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let commission = calculate_maker_taker_commission(
            &instrument,
            Quantity::from(100_000),
            Price::from("1.0"),
            dec!(0.0002),
            None,
        )
        .unwrap();
        let notional = instrument
            .try_calculate_notional_value(Quantity::from(100_000), Price::from("1.0"), None)
            .unwrap();
        assert_eq!(
            commission.as_decimal(),
            notional.as_decimal() * dec!(0.0002)
        );
        assert_eq!(commission.currency, notional.currency);
    }
}

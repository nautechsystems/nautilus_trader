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

use std::hash::{Hash, Hasher};

use nautilus_core::{
    UnixNanos,
    correctness::{
        FAILED, check_equal_u8, check_valid_string_ascii, check_valid_string_ascii_optional,
    },
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Instrument, any::InstrumentAny};
use crate::{
    enums::{AssetClass, InstrumentClass, OptionKind},
    identifiers::{InstrumentId, Symbol},
    types::{
        currency::Currency,
        money::Money,
        price::{Price, check_positive_price},
        quantity::{Quantity, check_positive_quantity},
    },
};

/// Represents a generic option spread instrument.
#[repr(C)]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OptionSpread {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The option spread asset class.
    pub asset_class: AssetClass,
    /// The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    pub exchange: Option<Ustr>,
    /// The underlying asset.
    pub underlying: Ustr,
    /// The strategy type of the spread.
    pub strategy_type: Ustr,
    /// UNIX timestamp (nanoseconds) for contract activation.
    pub activation_ns: UnixNanos,
    /// UNIX timestamp (nanoseconds) for contract expiration.
    pub expiration_ns: UnixNanos,
    /// The option spread currency.
    pub currency: Currency,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The option multiplier.
    pub multiplier: Quantity,
    /// The rounded lot unit size (standard/board).
    pub lot_size: Quantity,
    /// The initial (order) margin requirement in percentage of order value.
    pub margin_init: Decimal,
    /// The maintenance (position) margin in percentage of position value.
    pub margin_maint: Decimal,
    /// The fee rate for liquidity makers as a percentage of order value.
    pub maker_fee: Decimal,
    /// The fee rate for liquidity takers as a percentage of order value.
    pub taker_fee: Decimal,
    /// The maximum allowable order quantity.
    pub max_quantity: Option<Quantity>,
    /// The minimum allowable order quantity.
    pub min_quantity: Option<Quantity>,
    /// The maximum allowable quoted price.
    pub max_price: Option<Price>,
    /// The minimum allowable quoted price.
    pub min_price: Option<Price>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl OptionSpread {
    /// Creates a new [`OptionSpread`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    /// # Errors
    ///
    /// Returns an error if any input validation fails.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        strategy_type: Ustr,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_valid_string_ascii_optional(exchange.map(|u| u.as_str()), stringify!(isin))?;
        check_valid_string_ascii(strategy_type.as_str(), stringify!(strategy_type))?;
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_positive_price(price_increment, stringify!(price_increment))?;
        check_positive_quantity(multiplier, stringify!(multiplier))?;
        check_positive_quantity(lot_size, stringify!(lot_size))?;

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            size_precision: 0,
            size_increment: Quantity::from("1"),
            multiplier,
            lot_size,
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            max_quantity,
            min_quantity: Some(min_quantity.unwrap_or(1.into())),
            max_price,
            min_price,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`OptionSpread`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any input parameter is invalid (see `new_checked`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        strategy_type: Ustr,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        )
        .expect(FAILED)
    }
}

impl PartialEq<Self> for OptionSpread {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for OptionSpread {}

impl Hash for OptionSpread {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for OptionSpread {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::OptionSpread(self)
    }

    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        self.asset_class
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::OptionSpread
    }
    fn underlying(&self) -> Option<Ustr> {
        Some(self.underlying)
    }

    fn base_currency(&self) -> Option<Currency> {
        None
    }

    fn quote_currency(&self) -> Currency {
        self.currency
    }

    fn settlement_currency(&self) -> Currency {
        self.currency
    }

    fn isin(&self) -> Option<Ustr> {
        None
    }

    fn option_kind(&self) -> Option<OptionKind> {
        None
    }

    fn exchange(&self) -> Option<Ustr> {
        self.exchange
    }

    fn strike_price(&self) -> Option<Price> {
        None
    }

    fn activation_ns(&self) -> Option<UnixNanos> {
        Some(self.activation_ns)
    }

    fn expiration_ns(&self) -> Option<UnixNanos> {
        Some(self.expiration_ns)
    }

    fn is_inverse(&self) -> bool {
        false
    }

    fn price_precision(&self) -> u8 {
        self.price_precision
    }

    fn size_precision(&self) -> u8 {
        0
    }

    fn price_increment(&self) -> Price {
        self.price_increment
    }

    fn size_increment(&self) -> Quantity {
        Quantity::from(1)
    }

    fn multiplier(&self) -> Quantity {
        self.multiplier
    }

    fn lot_size(&self) -> Option<Quantity> {
        Some(self.lot_size)
    }

    fn max_quantity(&self) -> Option<Quantity> {
        self.max_quantity
    }

    fn min_quantity(&self) -> Option<Quantity> {
        self.min_quantity
    }

    fn max_notional(&self) -> Option<Money> {
        None
    }

    fn min_notional(&self) -> Option<Money> {
        None
    }

    fn max_price(&self) -> Option<Price> {
        self.max_price
    }

    fn min_price(&self) -> Option<Price> {
        self.min_price
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{OptionSpread, stubs::*};

    #[rstest]
    fn test_equality(option_spread: OptionSpread) {
        assert_eq!(option_spread, option_spread.clone());
    }
}

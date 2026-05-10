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

use std::hash::{Hash, Hasher};

use nautilus_core::{
    Params, UnixNanos,
    correctness::{CorrectnessResult, CorrectnessResultExt, FAILED, check_equal_u8},
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

/// Represents a generic binary option instrument.
#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BinaryOption {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The binary option asset class.
    pub asset_class: AssetClass,
    /// The binary option contract currency.
    pub currency: Currency,
    /// UNIX timestamp (nanoseconds) for contract activation.
    pub activation_ns: UnixNanos,
    /// UNIX timestamp (nanoseconds) for contract expiration.
    pub expiration_ns: UnixNanos,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// The initial (order) margin requirement in percentage of order value.
    pub margin_init: Decimal,
    /// The maintenance (position) margin in percentage of position value.
    pub margin_maint: Decimal,
    /// The fee rate for liquidity makers as a percentage of order value.
    pub maker_fee: Decimal,
    /// The fee rate for liquidity takers as a percentage of order value.
    pub taker_fee: Decimal,
    /// The binary outcome of the market.
    pub outcome: Option<Ustr>,
    /// The market description.
    pub description: Option<Ustr>,
    /// The maximum allowable order quantity.
    pub max_quantity: Option<Quantity>,
    /// The minimum allowable order quantity.
    pub min_quantity: Option<Quantity>,
    /// The maximum allowable order notional value.
    pub max_notional: Option<Money>,
    /// The minimum allowable order notional value.
    pub min_notional: Option<Money>,
    /// The maximum allowable quoted price.
    pub max_price: Option<Price>,
    /// The minimum allowable quoted price.
    pub min_price: Option<Price>,
    /// Additional instrument metadata as a JSON-serializable dictionary.
    pub info: Option<Params>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl BinaryOption {
    /// Creates a new [`BinaryOption`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    /// # Errors
    ///
    /// Returns an error if any input validation fails (e.g., invalid precision or increments).
    #[expect(clippy::too_many_arguments)]
    pub fn new_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        outcome: Option<Ustr>,
        description: Option<Ustr>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> CorrectnessResult<Self> {
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_equal_u8(
            size_precision,
            size_increment.precision,
            stringify!(size_precision),
            stringify!(size_increment.precision),
        )?;
        check_positive_price(price_increment, stringify!(price_increment))?;
        check_positive_quantity(size_increment, stringify!(size_increment))?;

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            outcome,
            description,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`BinaryOption`] instance by validating parameters.
    ///
    /// # Panics
    ///
    /// Panics if parameter validation fails during `new_checked`.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        outcome: Option<Ustr>,
        description: Option<Ustr>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            outcome,
            description,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            info,
            ts_event,
            ts_init,
        )
        .expect_display(FAILED)
    }
}

impl PartialEq<Self> for BinaryOption {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for BinaryOption {}

impl Hash for BinaryOption {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for BinaryOption {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::BinaryOption(self)
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
        InstrumentClass::BinaryOption
    }

    fn underlying(&self) -> Option<Ustr> {
        None
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

    fn exchange(&self) -> Option<Ustr> {
        None
    }

    fn option_kind(&self) -> Option<OptionKind> {
        None
    }

    fn is_inverse(&self) -> bool {
        false
    }

    fn price_precision(&self) -> u8 {
        self.price_precision
    }

    fn size_precision(&self) -> u8 {
        self.size_precision
    }

    fn price_increment(&self) -> Price {
        self.price_increment
    }

    fn size_increment(&self) -> Quantity {
        self.size_increment
    }

    fn multiplier(&self) -> Quantity {
        Quantity::from(1)
    }

    fn lot_size(&self) -> Option<Quantity> {
        Some(Quantity::from(1))
    }

    fn max_quantity(&self) -> Option<Quantity> {
        self.max_quantity
    }

    fn min_quantity(&self) -> Option<Quantity> {
        self.min_quantity
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

    fn margin_init(&self) -> Decimal {
        self.margin_init
    }

    fn margin_maint(&self) -> Decimal {
        self.margin_maint
    }

    fn maker_fee(&self) -> Decimal {
        self.maker_fee
    }

    fn taker_fee(&self) -> Decimal {
        self.taker_fee
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

    fn max_notional(&self) -> Option<Money> {
        self.max_notional
    }

    fn min_notional(&self) -> Option<Money> {
        self.min_notional
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        enums::{AssetClass, InstrumentClass},
        identifiers::{InstrumentId, Symbol},
        instruments::{BinaryOption, Instrument, stubs::*},
        types::{Currency, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(binary_option: BinaryOption) {
        assert_eq!(binary_option.asset_class(), AssetClass::Alternative);
        assert_eq!(
            binary_option.instrument_class(),
            InstrumentClass::BinaryOption
        );
        assert_eq!(binary_option.quote_currency(), Currency::USDC());
        assert!(!binary_option.is_inverse());
        assert_eq!(binary_option.price_precision(), 3);
        assert_eq!(binary_option.size_precision(), 2);
        assert!(binary_option.activation_ns().is_some());
        assert!(binary_option.expiration_ns().is_some());
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = BinaryOption::new_checked(
            InstrumentId::from("TEST.POLYMARKET"),
            Symbol::from("TEST"),
            AssetClass::Alternative,
            Currency::USDC(),
            0.into(),
            0.into(),
            4, // mismatch
            2,
            Price::from("0.001"),
            Quantity::from("0.01"),
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
            None,
            0.into(),
            0.into(),
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serialization_roundtrip(binary_option: BinaryOption) {
        let json = serde_json::to_string(&binary_option).unwrap();
        let deserialized: BinaryOption = serde_json::from_str(&json).unwrap();
        assert_eq!(binary_option, deserialized);
    }
}

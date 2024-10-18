// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    correctness::{check_equal_u8, check_positive_i64, check_positive_u64, FAILED},
    nanos::UnixNanos,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{any::InstrumentAny, Instrument};
use crate::{
    enums::{AssetClass, InstrumentClass, OptionKind},
    identifiers::{InstrumentId, Symbol},
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

/// Represents a generic binary option instrument.
#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct BinaryOption {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    pub asset_class: AssetClass,
    pub currency: Currency,
    pub activation_ns: UnixNanos,
    pub expiration_ns: UnixNanos,
    pub price_precision: u8,
    pub size_precision: u8,
    pub price_increment: Price,
    pub size_increment: Quantity,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub outcome: Option<Ustr>,
    pub description: Option<Ustr>,
    pub margin_init: Option<Decimal>,
    pub margin_maint: Option<Decimal>,
    pub max_quantity: Option<Quantity>,
    pub min_quantity: Option<Quantity>,
    pub max_notional: Option<Money>,
    pub min_notional: Option<Money>,
    pub max_price: Option<Price>,
    pub min_price: Option<Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl BinaryOption {
    /// Creates a new [`BinaryOption`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        outcome: Option<Ustr>,
        description: Option<Ustr>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
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
        check_positive_i64(price_increment.raw, stringify!(price_increment.raw))?;
        check_positive_u64(size_increment.raw, stringify!(size_increment.raw))?;

        Ok(Self {
            id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            maker_fee,
            taker_fee,
            outcome,
            description,
            margin_init,
            margin_maint,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            ts_event,
            ts_init,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        outcome: Option<Ustr>,
        description: Option<Ustr>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            maker_fee,
            taker_fee,
            outcome,
            description,
            margin_init,
            margin_maint,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            ts_event,
            ts_init,
        )
        .expect(FAILED)
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

    fn quote_currency(&self) -> Currency {
        self.currency
    }

    fn base_currency(&self) -> Option<Currency> {
        None
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{binary_option::BinaryOption, stubs::*};

    #[rstest]
    fn test_equality(binary_option: BinaryOption) {
        let cloned = binary_option.clone();
        assert_eq!(binary_option, cloned);
    }
}

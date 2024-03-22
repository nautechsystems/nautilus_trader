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

use std::{
    any::Any,
    hash::{Hash, Hasher},
};

use nautilus_core::{
    correctness::{check_equal_u8, check_positive_i64, check_valid_string_optional},
    time::UnixNanos,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::Instrument;
use crate::{
    enums::{AssetClass, InstrumentClass},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    types::{currency::Currency, price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct Equity {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    /// The ISIN (International Securities Identification Number).
    pub isin: Option<Ustr>,
    pub currency: Currency,
    pub price_precision: u8,
    pub price_increment: Price,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub margin_init: Decimal,
    pub margin_maint: Decimal,
    pub lot_size: Option<Quantity>,
    pub max_quantity: Option<Quantity>,
    pub min_quantity: Option<Quantity>,
    pub max_price: Option<Price>,
    pub min_price: Option<Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl Equity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        isin: Option<Ustr>,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_valid_string_optional(isin.map(|u| u.as_str()), stringify!(isin))?;
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_positive_i64(price_increment.raw, stringify!(price_increment.raw))?;

        Ok(Self {
            id,
            raw_symbol,
            isin,
            currency,
            price_precision,
            price_increment,
            maker_fee: maker_fee.unwrap_or(0.into()),
            taker_fee: taker_fee.unwrap_or(0.into()),
            margin_init: margin_init.unwrap_or(0.into()),
            margin_maint: margin_maint.unwrap_or(0.into()),
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            ts_event,
            ts_init,
        })
    }
}

impl PartialEq<Self> for Equity {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Equity {}

impl Hash for Equity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for Equity {
    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Equity
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::Spot
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
        Quantity::from(1)
    }

    fn lot_size(&self) -> Option<Quantity> {
        self.lot_size
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

    fn as_any(&self) -> &dyn Any {
        self
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{equity::Equity, stubs::*};

    #[rstest]
    fn test_equality(equity_aapl: Equity) {
        let cloned = equity_aapl;
        assert_eq!(equity_aapl, cloned);
    }
}

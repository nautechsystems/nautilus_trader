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

use anyhow::Result;
use nautilus_core::time::UnixNanos;
use pyo3::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

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
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct CurrencyPair {
    #[pyo3(get)]
    pub id: InstrumentId,
    #[pyo3(get)]
    pub raw_symbol: Symbol,
    #[pyo3(get)]
    pub base_currency: Currency,
    #[pyo3(get)]
    pub quote_currency: Currency,
    #[pyo3(get)]
    pub price_precision: u8,
    #[pyo3(get)]
    pub size_precision: u8,
    #[pyo3(get)]
    pub price_increment: Price,
    #[pyo3(get)]
    pub size_increment: Quantity,
    #[pyo3(get)]
    pub maker_fee: Decimal,
    #[pyo3(get)]
    pub taker_fee: Decimal,
    #[pyo3(get)]
    pub margin_init: Decimal,
    #[pyo3(get)]
    pub margin_maint: Decimal,
    #[pyo3(get)]
    pub lot_size: Option<Quantity>,
    #[pyo3(get)]
    pub max_quantity: Option<Quantity>,
    #[pyo3(get)]
    pub min_quantity: Option<Quantity>,
    #[pyo3(get)]
    pub max_price: Option<Price>,
    #[pyo3(get)]
    pub min_price: Option<Price>,
    #[pyo3(get)]
    pub ts_event: UnixNanos,
    #[pyo3(get)]
    pub ts_init: UnixNanos,
}

impl CurrencyPair {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        taker_fee: Decimal,
        maker_fee: Decimal,
        margin_init: Decimal,
        margin_maint: Decimal,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Result<Self> {
        Ok(Self {
            id,
            raw_symbol,
            quote_currency,
            base_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            taker_fee,
            maker_fee,
            margin_init,
            margin_maint,
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

impl PartialEq<Self> for CurrencyPair {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CurrencyPair {}

impl Hash for CurrencyPair {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for CurrencyPair {
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn raw_symbol(&self) -> &Symbol {
        &self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::FX
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::Spot
    }

    fn quote_currency(&self) -> &Currency {
        &self.quote_currency
    }

    fn base_currency(&self) -> Option<&Currency> {
        Some(&self.base_currency)
    }

    fn settlement_currency(&self) -> &Currency {
        &self.quote_currency
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
        // SAFETY: Unwrap safe as using known values
        Quantity::new(1.0, 0).unwrap()
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
    fn margin_init(&self) -> Decimal {
        self.margin_init
    }
    fn margin_maint(&self) -> Decimal {
        self.margin_maint
    }
    fn taker_fee(&self) -> Decimal {
        self.taker_fee
    }
    fn maker_fee(&self) -> Decimal {
        self.maker_fee
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
///////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{currency_pair::CurrencyPair, stubs::*};

    #[rstest]
    fn test_equality(currency_pair_btcusdt: CurrencyPair) {
        let cloned = currency_pair_btcusdt.clone();
        assert_eq!(currency_pair_btcusdt, cloned)
    }
}

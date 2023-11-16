// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(dead_code)] // Allow for development

use std::hash::{Hash, Hasher};

use anyhow::Result;
use pyo3::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::Instrument;
use crate::{
    enums::{AssetClass, AssetType},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    types::{currency::Currency, price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Equity {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    /// The instruments ISIN (International Securities Identification Number).
    pub isin: String,
    pub currency: Currency,
    pub price_precision: u8,
    pub price_increment: Price,
    pub multiplier: Quantity,
    pub margin_init: Decimal,
    pub margin_maint: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub lot_size: Option<Quantity>,
    pub max_quantity: Option<Quantity>,
    pub min_quantity: Option<Quantity>,
    pub max_price: Option<Price>,
    pub min_price: Option<Price>,
}

impl Equity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        isin: String,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        margin_init: Decimal,
        margin_maint: Decimal,
        maker_fee: Decimal,
        taker_fee: Decimal,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
    ) -> Result<Self> {
        Ok(Self {
            id,
            raw_symbol,
            isin,
            currency,
            price_precision,
            price_increment,
            multiplier,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
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
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn raw_symbol(&self) -> &Symbol {
        &self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Equity
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Spot
    }

    fn quote_currency(&self) -> &Currency {
        &self.currency
    }

    fn base_currency(&self) -> Option<&Currency> {
        None
    }

    fn settlement_currency(&self) -> &Currency {
        &self.currency
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
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use std::str::FromStr;

    use rstest::fixture;
    use rust_decimal::Decimal;

    use crate::{
        identifiers::{instrument_id::InstrumentId, symbol::Symbol},
        instruments::equity::Equity,
        types::{currency::Currency, price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn equity_aapl() -> Equity {
        Equity::new(
            InstrumentId::from("AAPL.NASDAQ"),
            Symbol::from("AAPL"),
            String::from("US0378331005"),
            Currency::from("USD"),
            2,
            Price::from("0.01"),
            Quantity::from(1),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            Some(Quantity::from(1)),
            None,
            None,
            None,
            None,
        )
        .unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::stubs::*;
    use crate::instruments::equity::Equity;

    #[rstest]
    fn test_equality(equity_aapl: Equity) {
        let cloned = equity_aapl.clone();
        assert_eq!(equity_aapl, cloned)
    }
}

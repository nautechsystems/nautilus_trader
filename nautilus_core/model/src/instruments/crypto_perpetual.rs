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

use crate::{
    enums::{AssetClass, AssetType},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    instruments::Instrument,
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct CryptoPerpetual {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    pub base_currency: Currency,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,
    pub price_precision: u8,
    pub size_precision: u8,
    pub price_increment: Price,
    pub size_increment: Quantity,
    pub margin_init: Decimal,
    pub margin_maint: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub lot_size: Option<Quantity>,
    pub max_quantity: Option<Quantity>,
    pub min_quantity: Option<Quantity>,
    pub max_notional: Option<Money>,
    pub min_notional: Option<Money>,
    pub max_price: Option<Price>,
    pub min_price: Option<Price>,
}

impl CryptoPerpetual {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        margin_init: Decimal,
        margin_maint: Decimal,
        maker_fee: Decimal,
        taker_fee: Decimal,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
    ) -> Result<Self> {
        Ok(Self {
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            settlement_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
        })
    }
}

impl PartialEq<Self> for CryptoPerpetual {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CryptoPerpetual {}

impl Hash for CryptoPerpetual {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for CryptoPerpetual {
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn raw_symbol(&self) -> &Symbol {
        &self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Cryptocurrency
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Swap
    }

    fn quote_currency(&self) -> &Currency {
        &self.quote_currency
    }

    fn base_currency(&self) -> Option<&Currency> {
        None
    }

    fn settlement_currency(&self) -> &Currency {
        &self.settlement_currency
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
        instruments::crypto_perpetual::CryptoPerpetual,
        types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn crypto_perpetual_ethusdt() -> CryptoPerpetual {
        CryptoPerpetual::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Symbol::from("ETHUSDT"),
            Currency::from("ETH"),
            Currency::from("USDT"),
            Currency::from("USDT"),
            2,
            0,
            Price::from("0.01"),
            Quantity::from("0.001"),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            None,
            Some(Quantity::from("10000.0")),
            Some(Quantity::from("0.001")),
            None,
            Some(Money::new(10.00, Currency::from("USDT")).unwrap()),
            Some(Price::from("15000.00")),
            Some(Price::from("1.0")),
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
    use crate::instruments::crypto_perpetual::CryptoPerpetual;

    #[rstest]
    fn test_equality(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let cloned = crypto_perpetual_ethusdt.clone();
        assert_eq!(crypto_perpetual_ethusdt, cloned)
    }
}

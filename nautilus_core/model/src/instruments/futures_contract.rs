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
use nautilus_core::time::UnixNanos;
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
pub struct FuturesContract {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    pub asset_class: AssetClass,
    pub underlying: String,
    pub expiration: UnixNanos,
    pub currency: Currency,
    pub price_precision: u8,
    pub price_increment: Price,
    pub margin_init: Decimal,
    pub margin_maint: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub multiplier: Quantity,
    pub lot_size: Option<Quantity>,
    pub max_quantity: Option<Quantity>,
    pub min_quantity: Option<Quantity>,
    pub max_price: Option<Price>,
    pub min_price: Option<Price>,
}

impl FuturesContract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: String,
        expiration: UnixNanos,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        margin_init: Decimal,
        margin_maint: Decimal,
        maker_fee: Decimal,
        taker_fee: Decimal,
        multiplier: Quantity,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
    ) -> Result<Self> {
        Ok(Self {
            id,
            raw_symbol,
            asset_class,
            underlying,
            expiration,
            currency,
            price_precision,
            price_increment,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
        })
    }
}

impl PartialEq<Self> for FuturesContract {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for FuturesContract {}

impl Hash for FuturesContract {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for FuturesContract {
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn raw_symbol(&self) -> &Symbol {
        &self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        self.asset_class
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Future
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

    use chrono::{TimeZone, Utc};
    use nautilus_core::time::UnixNanos;
    use rstest::fixture;
    use rust_decimal::Decimal;

    use crate::{
        enums::AssetClass,
        identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
        instruments::futures_contract::FuturesContract,
        types::{currency::Currency, price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn futures_contract_es() -> FuturesContract {
        let expiration = Utc.with_ymd_and_hms(2021, 7, 8, 0, 0, 0).unwrap();
        FuturesContract::new(
            InstrumentId::new(Symbol::from("ESZ21"), Venue::from("CME")),
            Symbol::from("ESZ21"),
            AssetClass::Index,
            String::from("ES"),
            expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
            Currency::USD(),
            2,
            Price::from("0.01"),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            Quantity::from("1.0"),
            Some(Quantity::from("1.0")),
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
    use crate::instruments::futures_contract::FuturesContract;

    #[rstest]
    fn test_equality(futures_contract_es: FuturesContract) {
        let cloned = futures_contract_es.clone();
        assert_eq!(futures_contract_es, cloned);
    }
}

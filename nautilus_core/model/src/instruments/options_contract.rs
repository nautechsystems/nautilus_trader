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
    enums::{AssetClass, AssetType, OptionKind},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    types::{currency::Currency, price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OptionsContract {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    pub asset_class: AssetClass,
    pub underlying: String,
    pub option_kind: OptionKind,
    pub activation_ns: UnixNanos,
    pub expiration_ns: UnixNanos,
    pub strike_price: Price,
    pub currency: Currency,
    pub price_precision: u8,
    pub price_increment: Price,
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

impl OptionsContract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: String,
        option_kind: OptionKind,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        strike_price: Price,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
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
            asset_class,
            underlying,
            option_kind,
            activation_ns,
            expiration_ns,
            strike_price,
            currency,
            price_precision,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
        })
    }
}

impl PartialEq<Self> for OptionsContract {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for OptionsContract {}

impl Hash for OptionsContract {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for OptionsContract {
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
        AssetType::Option
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
        enums::{AssetClass, OptionKind},
        identifiers::{instrument_id::InstrumentId, symbol::Symbol},
        instruments::options_contract::OptionsContract,
        types::{currency::Currency, price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn options_contract_appl() -> OptionsContract {
        let activation = Utc.with_ymd_and_hms(2021, 9, 17, 0, 0, 0).unwrap();
        let expiration = Utc.with_ymd_and_hms(2021, 12, 17, 0, 0, 0).unwrap();
        OptionsContract::new(
            InstrumentId::from("AAPL211217C00150000.OPRA"),
            Symbol::from("AAPL211217C00150000"),
            AssetClass::Equity,
            String::from("AAPL"),
            OptionKind::Call,
            activation.timestamp_nanos_opt().unwrap() as UnixNanos,
            expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
            Price::from("149.0"),
            Currency::USD(),
            2,
            Price::from("0.01"),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.0").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            Decimal::from_str("0.001").unwrap(),
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
    use crate::instruments::options_contract::OptionsContract;

    #[rstest]
    fn test_equality(options_contract_appl: OptionsContract) {
        let options_contract_appl2 = options_contract_appl.clone();
        assert_eq!(options_contract_appl, options_contract_appl2);
    }
}

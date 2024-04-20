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
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    correctness::{
        check_equal_u8, check_positive_i64, check_valid_string, check_valid_string_optional,
    },
    nanos::UnixNanos,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, FromRow, Row};
use ustr::Ustr;

use super::{Instrument, InstrumentAny};
use crate::{
    enums::{AssetClass, InstrumentClass, OptionKind},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct OptionsContract {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    pub asset_class: AssetClass,
    /// The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    pub exchange: Option<Ustr>,
    pub underlying: Ustr,
    pub option_kind: OptionKind,
    pub activation_ns: UnixNanos,
    pub expiration_ns: UnixNanos,
    pub strike_price: Price,
    pub currency: Currency,
    pub price_precision: u8,
    pub price_increment: Price,
    pub size_increment: Quantity,
    pub size_precision: u8,
    pub multiplier: Quantity,
    pub lot_size: Quantity,
    pub margin_init: Decimal,
    pub margin_maint: Decimal,
    pub max_quantity: Option<Quantity>,
    pub min_quantity: Option<Quantity>,
    pub max_price: Option<Price>,
    pub min_price: Option<Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OptionsContract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        option_kind: OptionKind,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        strike_price: Price,
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
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_valid_string_optional(exchange.map(|u| u.as_str()), stringify!(isin))?;
        check_valid_string(underlying.as_str(), stringify!(underlying))?;
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
            asset_class,
            exchange,
            underlying,
            option_kind,
            activation_ns,
            expiration_ns,
            strike_price,
            currency,
            price_precision,
            price_increment,
            size_precision: 0,
            size_increment: Quantity::from("1"),
            multiplier,
            lot_size,
            max_quantity,
            min_quantity: Some(min_quantity.unwrap_or(1.into())),
            max_price,
            min_price,
            margin_init: margin_init.unwrap_or(0.into()),
            margin_maint: margin_maint.unwrap_or(0.into()),
            ts_event,
            ts_init,
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
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::OptionsContract(self)
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
        InstrumentClass::Option
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
        Some(self.option_kind)
    }

    fn exchange(&self) -> Option<Ustr> {
        self.exchange
    }

    fn strike_price(&self) -> Option<Price> {
        Some(self.strike_price)
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

impl<'r> FromRow<'r, PgRow> for OptionsContract {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::new(res.as_str()).unwrap())?;
        let asset_class = row
            .try_get::<String, _>("asset_class")
            .map(|res| AssetClass::from_str(res.as_str()).unwrap())?;
        let exchange = row
            .try_get::<Option<String>, _>("exchange")
            .map(|res| res.map(|s| Ustr::from(s.as_str())))?;
        let underlying = row
            .try_get::<String, _>("underlying")
            .map(|res| Ustr::from(res.as_str()))?;
        let option_kind = row
            .try_get::<String, _>("option_kind")
            .map(|res| OptionKind::from_str(res.as_str()).unwrap())?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let strike_price = row
            .try_get::<String, _>("strike_price")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from_str(res.as_str()).unwrap())?;
        let price_precision = row.try_get::<i32, _>("price_precision").unwrap();
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let multiplier = row
            .try_get::<String, _>("multiplier")
            .map(|res| Quantity::from(res.as_str()))?;
        let lot_size = row
            .try_get::<String, _>("lot_size")
            .map(|res| Quantity::from(res.as_str()))
            .unwrap();
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from(s.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from(s.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            option_kind,
            activation_ns,
            expiration_ns,
            strike_price,
            currency,
            price_precision as u8,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            Some(margin_init),
            Some(margin_maint),
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{options_contract::OptionsContract, stubs::*};

    #[rstest]
    fn test_equality(options_contract_appl: OptionsContract) {
        let options_contract_appl2 = options_contract_appl;
        assert_eq!(options_contract_appl, options_contract_appl2);
    }
}

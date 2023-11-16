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

pub mod crypto_future;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod options_contract;
pub mod synthetic;
pub mod synthetic_api;

use anyhow::Result;
use rust_decimal::Decimal;

use crate::{
    enums::{AssetClass, AssetType},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

pub trait Instrument {
    fn id(&self) -> &InstrumentId;
    fn symbol(&self) -> &Symbol {
        &self.id().symbol
    }
    fn venue(&self) -> &Venue {
        &self.id().venue
    }
    fn raw_symbol(&self) -> &Symbol;
    fn asset_class(&self) -> AssetClass;
    fn asset_type(&self) -> AssetType;
    fn base_currency(&self) -> Option<&Currency>;
    fn quote_currency(&self) -> &Currency;
    fn settlement_currency(&self) -> &Currency;
    fn is_inverse(&self) -> bool;
    fn price_precision(&self) -> u8;
    fn size_precision(&self) -> u8;
    fn price_increment(&self) -> Price;
    fn size_increment(&self) -> Quantity;
    fn multiplier(&self) -> Quantity;
    fn lot_size(&self) -> Option<Quantity>;
    fn max_quantity(&self) -> Option<Quantity>;
    fn min_quantity(&self) -> Option<Quantity>;
    fn max_price(&self) -> Option<Price>;
    fn min_price(&self) -> Option<Price>;
    fn margin_init(&self) -> Decimal;
    fn margin_maint(&self) -> Decimal;
    fn maker_fee(&self) -> Decimal;
    fn taker_fee(&self) -> Decimal;

    /// Creates a new price from the given `value` with the correct price precision for the instrument.
    fn make_price(&self, value: f64) -> Result<Price> {
        Price::new(value, self.price_precision())
    }

    /// Creates a new quantity from the given `value` with the correct size precision for the instrument.
    fn make_qty(&self, value: f64) -> Result<Quantity> {
        Quantity::new(value, self.size_precision())
    }

    /// Calculates the notional value from the given parameters.
    /// The `use_quote_for_inverse` flag is only applicable for inverse instruments.
    ///
    /// # Panics
    ///
    /// If instrument is inverse and not `use_quote_for_inverse`, with no base currency.
    fn calculate_notional_value(
        &self,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> Money {
        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        let (amount, currency) = if self.is_inverse() {
            if use_quote_for_inverse {
                (quantity.as_f64(), self.quote_currency().to_owned())
            } else {
                let amount =
                    quantity.as_f64() * self.multiplier().as_f64() * (1.0 / price.as_f64());
                let currency = self
                    .base_currency()
                    .expect("Error: no base currency for notional calculation")
                    .to_owned();
                (amount, currency)
            }
        } else {
            let amount = quantity.as_f64() * self.multiplier().as_f64() * price.as_f64();
            let currency = self.quote_currency().to_owned();
            (amount, currency)
        };

        Money::new(amount, currency).unwrap() // TODO: Handle error properly
    }

    /// Returns the equivalent quantity of the base asset.
    fn calculate_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        let value = quantity.as_f64() * (1.0 / last_px.as_f64());
        Quantity::new(value, self.size_precision()).unwrap() // TODO: Handle error properly
    }
}

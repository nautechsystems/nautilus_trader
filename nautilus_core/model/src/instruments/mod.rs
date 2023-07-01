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

mod currency_pair;
mod synthetic;
mod synthetic_api;

use rust_decimal::Decimal;

use crate::{
    enums::{AssetClass, AssetType},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol},
    types::{currency::Currency, price::Price, quantity::Quantity},
};

pub trait Instrument {
    fn id(&self) -> &InstrumentId;
    fn native_symbol(&self) -> &Symbol;
    fn asset_class(&self) -> AssetClass;
    fn asset_type(&self) -> AssetType;
    fn quote_currency(&self) -> &Currency;
    fn base_currency(&self) -> Option<&Currency>;
    fn cost_currency(&self) -> &Currency;
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
}

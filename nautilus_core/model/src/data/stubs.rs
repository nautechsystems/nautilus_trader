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

use rstest::fixture;

use super::OrderBookDelta;
use crate::{
    data::order::BookOrder,
    enums::{BookAction, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

#[fixture]
pub fn stub_delta() -> OrderBookDelta {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let action = BookAction::Add;
    let price = Price::from("100.00");
    let size = Quantity::from("10");
    let side = OrderSide::Buy;
    let order_id = 123_456;
    let flags = 0;
    let sequence = 1;
    let ts_event = 1;
    let ts_init = 2;

    let order = BookOrder::new(side, price, size, order_id);
    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

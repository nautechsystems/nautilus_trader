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

use nautilus_core::uuid::UUID4;
use rstest::fixture;
use ustr::Ustr;

use crate::{
    events::order::OrderDenied,
    identifiers::{
        client_order_id::ClientOrderId, instrument_id::InstrumentId, strategy_id::StrategyId,
        stubs::*, trader_id::TraderId,
    },
};

#[fixture]
pub fn order_denied_max_submitted_rate(
    trader_id: TraderId,
    strategy_id_ema_cross: StrategyId,
    instrument_id_btc_usdt: InstrumentId,
    client_order_id: ClientOrderId,
) -> OrderDenied {
    let event_id = UUID4::new();
    OrderDenied::new(
        trader_id,
        strategy_id_ema_cross,
        instrument_id_btc_usdt,
        client_order_id,
        Ustr::from("Exceeded MAX_ORDER_SUBMIT_RATE"),
        event_id,
        0,
        0,
    )
    .unwrap()
}

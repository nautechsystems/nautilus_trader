// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::UnixNanos;
use nautilus_model::{
    events::OrderEventAny,
    orders::{Order, OrderAny},
};

pub(super) fn transform_order(
    order: &OrderAny,
    event: OrderEventAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderAny> {
    let history = order.events();
    anyhow::ensure!(
        !history.contains(&&event),
        "Duplicate initialization in legacy order history"
    );
    let mut transformed = OrderAny::from_events(vec![event])?;
    anyhow::ensure!(
        (
            transformed.trader_id(),
            transformed.strategy_id(),
            transformed.instrument_id(),
            transformed.client_order_id()
        ) == (
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id()
        ),
        "Legacy order transformation changes the order identity"
    );
    let history = history.into_iter().cloned();

    match &mut transformed {
        OrderAny::Market(target) => {
            target.ts_init = ts_init;
            target.prepend_events(history);
        }
        OrderAny::Limit(target) => {
            target.ts_init = ts_init;
            target.liquidity_side = order.liquidity_side();
            target.prepend_events(history);
        }
        _ => anyhow::bail!("Legacy order transformation must produce a market or limit order"),
    }

    Ok(transformed)
}

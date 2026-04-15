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

use std::convert::TryInto;

use ibapi::orders::{Order as IBOrder, TimeInForce};
use nautilus_model::{
    enums::{OrderType as NautilusOrderType, TrailingOffsetType},
    instruments::Instrument,
    orders::{Order as NautilusOrder, any::OrderAny},
};

use super::{convert_price, format_ib_datetime, trigger_type_to_ib_trigger_method};
use crate::providers::instruments::InteractiveBrokersInstrumentProvider;

pub(super) fn apply_expire_time_policy(ib_order: &mut IBOrder, order: &OrderAny) {
    if matches!(ib_order.tif, TimeInForce::GoodTilDate)
        && let Some(expire) = order.expire_time()
    {
        ib_order.good_till_date = format_ib_datetime(expire);
    }
}

pub(super) fn apply_account_policy(ib_order: &mut IBOrder, order: &OrderAny) {
    if let Some(account_id) = order.account_id() {
        ib_order.account = account_id.to_string();
    }
}

pub(super) fn apply_quantity_policy(
    ib_order: &mut IBOrder,
    order: &OrderAny,
    instrument_provider: &InteractiveBrokersInstrumentProvider,
) {
    if let Some(instrument) = instrument_provider.find(&order.instrument_id())
        && instrument.is_inverse()
        && order.is_quote_quantity()
    {
        ib_order.cash_qty = Some(order.quantity().as_f64());
        ib_order.total_quantity = 0.0;
    }
}

pub(super) fn apply_trailing_order_policy(
    ib_order: &mut IBOrder,
    order: &OrderAny,
    price_magnifier: f64,
) -> anyhow::Result<()> {
    if !matches!(
        order.order_type(),
        NautilusOrderType::TrailingStopMarket | NautilusOrderType::TrailingStopLimit
    ) {
        return Ok(());
    }

    match order.trailing_offset_type() {
        Some(TrailingOffsetType::Price | TrailingOffsetType::NoTrailingOffset) | None => {}
        Some(other) => anyhow::bail!(
            "`TrailingOffsetType` {:?} is not supported (only PRICE is supported)",
            other
        ),
    }

    if let Some(trailing_offset) = order.trailing_offset() {
        ib_order.aux_price = Some(
            trailing_offset
                .try_into()
                .unwrap_or_else(|_| trailing_offset.to_string().parse::<f64>().unwrap_or(0.0)),
        );
    }

    if let Some(trigger_price) = order.trigger_price() {
        let converted_trigger = convert_price(trigger_price, price_magnifier);
        ib_order.trail_stop_price = Some(converted_trigger);
        ib_order.trigger_method = order
            .trigger_type()
            .map(trigger_type_to_ib_trigger_method)
            .unwrap_or_default();
    }

    Ok(())
}

pub(super) fn apply_display_quantity_policy(ib_order: &mut IBOrder, order: &OrderAny) {
    if let Some(display_qty) = order.display_qty() {
        ib_order.display_size = Some(display_qty.as_f64() as i32);
    }
}

pub(super) fn apply_order_list_policy(ib_order: &mut IBOrder, order: &OrderAny) {
    if let Some(order_list_id) = order.order_list_id()
        && ib_order.oca_group.is_empty()
    {
        ib_order.oca_group = order_list_id.to_string();
    }
}

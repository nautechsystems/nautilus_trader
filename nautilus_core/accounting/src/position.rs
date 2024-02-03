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

use std::collections::HashMap;

use anyhow::Result;
use nautilus_core::time::UnixNanos;
use nautilus_model::enums::{OrderSide, PositionSide};
use nautilus_model::events::order::filled::OrderFilled;
use nautilus_model::identifiers::account_id::AccountId;
use nautilus_model::identifiers::client_order_id::ClientOrderId;
use nautilus_model::identifiers::instrument_id::InstrumentId;
use nautilus_model::identifiers::position_id::PositionId;
use nautilus_model::identifiers::strategy_id::StrategyId;
use nautilus_model::identifiers::trade_id::TradeId;
use nautilus_model::identifiers::trader_id::TraderId;
use nautilus_model::identifiers::venue_order_id::VenueOrderId;
use nautilus_model::instruments::Instrument;
use nautilus_model::types::currency::Currency;
use nautilus_model::types::money::Money;
use nautilus_model::types::quantity::Quantity;
use pyo3::prelude::*;

/// Represents a position in a financial market.
///
/// The position ID may be assigned at the trading venue, or can be system
/// generated depending on a strategies OMS (Order Management System) settings.
#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct Position {
    events: Vec<OrderFilled>,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub closing_order_id: Option<ClientOrderId>,
    pub entry: OrderSide,
    pub side: PositionSide,
    pub signed_qty: f64,
    pub quantity: Quantity,
    pub peak_qty: Quantity,
    pub price_precision: u8,
    pub size_precision: u8,
    pub multiplier: Quantity,
    pub is_inverse: bool,
    pub base_currency: Option<Currency>,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,
    pub ts_init: UnixNanos,
    pub ts_opened: UnixNanos,
    pub ts_last: UnixNanos,
    pub ts_closed: Option<UnixNanos>,
    pub duration_ns: u64,
    pub avg_px_open: f64,
    pub avg_px_close: Option<f64>,
    pub realized_return: Option<f64>,
    pub realized_pnl: Option<Money>,
    venue_order_ids: Vec<VenueOrderId>,
    trade_ids: Vec<TradeId>,
    buy_qty: Quantity,
    sell_qty: Quantity,
    commissions: HashMap<Currency, Money>,
}

impl Position {
    pub fn new<T: Instrument>(instrument: T, fill: OrderFilled) -> Result<Self> {
        assert_eq!(instrument.id(), &fill.instrument_id);
        assert!(fill.position_id.is_some());
        assert_ne!(fill.order_side, OrderSide::NoOrderSide);

        let item = Self {
            events: Vec::<OrderFilled>::new(),
            venue_order_ids: Vec::<VenueOrderId>::new(),
            trade_ids: Vec::<TradeId>::new(),
            buy_qty: Quantity::zero(instrument.size_precision()),
            sell_qty: Quantity::zero(instrument.size_precision()),
            commissions: HashMap::<Currency, Money>::new(),
            trader_id: fill.trader_id,
            strategy_id: fill.strategy_id,
            instrument_id: fill.instrument_id,
            id: fill.position_id.unwrap(), // TODO: Improve validation
            account_id: fill.account_id,
            opening_order_id: fill.client_order_id,
            closing_order_id: None,
            entry: fill.order_side,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: fill.last_qty,
            peak_qty: fill.last_qty,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
            multiplier: instrument.multiplier(),
            is_inverse: instrument.is_inverse(),
            base_currency: instrument.base_currency().copied(),
            quote_currency: *instrument.quote_currency(),
            settlement_currency: *instrument.settlement_currency(),
            ts_init: fill.ts_init,
            ts_opened: fill.ts_event,
            ts_last: fill.ts_event,
            ts_closed: None,
            duration_ns: 0,
            avg_px_open: fill.last_px.as_f64(),
            avg_px_close: None,
            realized_return: None,
            realized_pnl: None,
        };
        Ok(item)
    }
}

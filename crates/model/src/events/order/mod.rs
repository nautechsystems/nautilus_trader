// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

pub mod accepted;
pub mod any;
pub mod cancel_rejected;
pub mod canceled;
pub mod denied;
pub mod emulated;
pub mod expired;
pub mod filled;
pub mod initialized;
pub mod modify_rejected;
pub mod pending_cancel;
pub mod pending_update;
pub mod rejected;
pub mod released;
pub mod snapshot;
pub mod submitted;
pub mod triggered;
pub mod updated;

#[cfg(feature = "stubs")]
pub mod stubs;

/// Represents a type of [`OrderEvent`].
#[derive(Debug, PartialEq, Eq)]
pub enum OrderEventType {
    Initialized,
    Denied,
    Emulated,
    Released,
    Submitted,
    Accepted,
    Rejected,
    Canceled,
    Expired,
    Triggered,
    PendingUpdate,
    PendingCancel,
    ModifyRejected,
    CancelRejected,
    Updated,
    PartiallyFilled,
    Filled,
}

pub trait OrderEvent: 'static + Send {
    fn id(&self) -> UUID4;
    fn kind(&self) -> &str;
    fn order_type(&self) -> Option<OrderType>;
    fn order_side(&self) -> Option<OrderSide>;
    fn trader_id(&self) -> TraderId;
    fn strategy_id(&self) -> StrategyId;
    fn instrument_id(&self) -> InstrumentId;
    fn trade_id(&self) -> Option<TradeId>;
    fn currency(&self) -> Option<Currency>;
    fn client_order_id(&self) -> ClientOrderId;
    fn reason(&self) -> Option<Ustr>;
    fn quantity(&self) -> Option<Quantity>;
    fn time_in_force(&self) -> Option<TimeInForce>;
    fn liquidity_side(&self) -> Option<LiquiditySide>;
    fn post_only(&self) -> Option<bool>;
    fn reduce_only(&self) -> Option<bool>;
    fn quote_quantity(&self) -> Option<bool>;
    fn reconciliation(&self) -> bool;
    fn price(&self) -> Option<Price>;
    fn last_px(&self) -> Option<Price>;
    fn last_qty(&self) -> Option<Quantity>;
    fn trigger_price(&self) -> Option<Price>;
    fn trigger_type(&self) -> Option<TriggerType>;
    fn limit_offset(&self) -> Option<Decimal>;
    fn trailing_offset(&self) -> Option<Decimal>;
    fn trailing_offset_type(&self) -> Option<TrailingOffsetType>;
    fn expire_time(&self) -> Option<UnixNanos>;
    fn display_qty(&self) -> Option<Quantity>;
    fn emulation_trigger(&self) -> Option<TriggerType>;
    fn trigger_instrument_id(&self) -> Option<InstrumentId>;
    fn contingency_type(&self) -> Option<ContingencyType>;
    fn order_list_id(&self) -> Option<OrderListId>;
    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>>;
    fn parent_order_id(&self) -> Option<ClientOrderId>;
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId>;
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
    fn venue_order_id(&self) -> Option<VenueOrderId>;
    fn account_id(&self) -> Option<AccountId>;
    fn position_id(&self) -> Option<PositionId>;
    fn commission(&self) -> Option<Money>;
    fn ts_event(&self) -> UnixNanos;
    fn ts_init(&self) -> UnixNanos;
}

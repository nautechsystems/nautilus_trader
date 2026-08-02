// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use ustr::Ustr;

use crate::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::OrderFillVoided,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    stubs::{TestDefault, test_uuid},
    types::{Currency, Money, Price, Quantity},
};

/// Test-only fluent spec for [`OrderFillVoided`].
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn = into_spec)]
pub struct OrderFillVoidedSpec {
    #[builder(default = TraderId::test_default())]
    pub trader_id: TraderId,
    #[builder(default = StrategyId::test_default())]
    pub strategy_id: StrategyId,
    #[builder(default = InstrumentId::test_default())]
    pub instrument_id: InstrumentId,
    #[builder(default = ClientOrderId::test_default())]
    pub client_order_id: ClientOrderId,
    #[builder(default = VenueOrderId::test_default())]
    pub venue_order_id: VenueOrderId,
    #[builder(default = AccountId::test_default())]
    pub account_id: AccountId,
    #[builder(default = Ustr::from("CORRECTION-001"))]
    pub correction_id: Ustr,
    #[builder(default = TradeId::test_default())]
    pub trade_id: TradeId,
    #[builder(default = Quantity::new(100_000.0, 0))]
    pub voided_qty: Quantity,
    pub commission_voided: Option<Money>,
    #[builder(default = OrderSide::Buy)]
    pub order_side: OrderSide,
    #[builder(default = OrderType::Market)]
    pub order_type: OrderType,
    #[builder(default = Price::from("1.00000"))]
    pub last_px: Price,
    #[builder(default = Currency::USD())]
    pub currency: Currency,
    #[builder(default = LiquiditySide::Taker)]
    pub liquidity_side: LiquiditySide,
    pub position_id: Option<PositionId>,
    pub reason: Option<Ustr>,
    pub info: Option<IndexMap<Ustr, Ustr>>,
    #[builder(default = test_uuid())]
    pub event_id: UUID4,
    #[builder(default = UnixNanos::default())]
    pub ts_event: UnixNanos,
    #[builder(default = UnixNanos::default())]
    pub ts_init: UnixNanos,
    #[builder(default = false)]
    pub reconciliation: bool,
    #[builder(default = false)]
    pub is_reopened: bool,
}

impl<S: order_fill_voided_spec_builder::IsComplete> OrderFillVoidedSpecBuilder<S> {
    /// Builds the spec through the production constructor.
    #[must_use]
    pub fn build(self) -> OrderFillVoided {
        let spec = self.into_spec();
        OrderFillVoided::new(
            spec.trader_id,
            spec.strategy_id,
            spec.instrument_id,
            spec.client_order_id,
            spec.venue_order_id,
            spec.account_id,
            spec.correction_id,
            spec.trade_id,
            spec.voided_qty,
            spec.commission_voided,
            spec.order_side,
            spec.order_type,
            spec.last_px,
            spec.currency,
            spec.liquidity_side,
            spec.position_id,
            spec.reason,
            spec.info,
            spec.event_id,
            spec.ts_event,
            spec.ts_init,
            spec.reconciliation,
            spec.is_reopened,
        )
    }
}

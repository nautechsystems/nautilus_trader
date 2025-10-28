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

// TODO: Under development
#![allow(dead_code)]

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    events::{OrderEventAny, OrderSubmitted},
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TradeId,
        TraderId,
    },
    orders::{
        Order, OrderAny, limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder,
        market::MarketOrder, market_if_touched::MarketIfTouchedOrder,
        market_to_limit::MarketToLimitOrder, stop_limit::StopLimitOrder,
        stop_market::StopMarketOrder, trailing_stop_limit::TrailingStopLimitOrder,
        trailing_stop_market::TrailingStopMarketOrder,
    },
    types::{Currency, Price, Quantity},
};

#[derive(Debug)]
pub struct OrderTestBuilder {
    kind: OrderType,
    trader_id: Option<TraderId>,
    strategy_id: Option<StrategyId>,
    instrument_id: Option<InstrumentId>,
    client_order_id: Option<ClientOrderId>,
    trade_id: Option<TradeId>,
    currency: Option<Currency>,
    side: Option<OrderSide>,
    quantity: Option<Quantity>,
    price: Option<Price>,
    trigger_price: Option<Price>,
    trigger_type: Option<TriggerType>,
    limit_offset: Option<Decimal>,
    trailing_offset: Option<Decimal>,
    trailing_offset_type: Option<TrailingOffsetType>,
    time_in_force: Option<TimeInForce>,
    expire_time: Option<UnixNanos>,
    reduce_only: Option<bool>,
    post_only: Option<bool>,
    quote_quantity: Option<bool>,
    reconciliation: Option<bool>,
    display_qty: Option<Quantity>,
    liquidity_side: Option<LiquiditySide>,
    emulation_trigger: Option<TriggerType>,
    trigger_instrument_id: Option<InstrumentId>,
    order_list_id: Option<OrderListId>,
    linked_order_ids: Option<Vec<ClientOrderId>>,
    parent_order_id: Option<ClientOrderId>,
    exec_algorithm_id: Option<ExecAlgorithmId>,
    exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
    exec_spawn_id: Option<ClientOrderId>,
    tags: Option<Vec<Ustr>>,
    init_id: Option<UUID4>,
    ts_init: Option<UnixNanos>,
    contingency_type: Option<ContingencyType>,
    submitted: bool,
}

impl OrderTestBuilder {
    /// Creates a new [`OrderTestBuilder`] instance.
    pub fn new(kind: OrderType) -> Self {
        Self {
            kind,
            trader_id: None,
            strategy_id: None,
            instrument_id: None,
            client_order_id: None,
            trade_id: None,
            currency: None,
            side: None,
            quantity: None,
            price: None,
            trigger_price: None,
            trigger_type: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
            time_in_force: None,
            contingency_type: None,
            expire_time: None,
            reduce_only: None,
            post_only: None,
            quote_quantity: None,
            reconciliation: None,
            display_qty: None,
            liquidity_side: None,
            emulation_trigger: None,
            trigger_instrument_id: None,
            linked_order_ids: None,
            order_list_id: None,
            parent_order_id: None,
            exec_algorithm_id: None,
            exec_algorithm_params: None,
            exec_spawn_id: None,
            init_id: None,
            ts_init: None,
            tags: None,
            submitted: false,
        }
    }

    pub fn submit(&mut self, submit: bool) -> &mut Self {
        self.submitted = submit;
        self
    }

    pub fn kind(&mut self, kind: OrderType) -> &mut Self {
        self.kind = kind;
        self
    }

    /// ----------- TraderId ----------
    pub fn trader_id(&mut self, trader_id: TraderId) -> &mut Self {
        self.trader_id = Some(trader_id);
        self
    }

    fn get_trader_id(&self) -> TraderId {
        self.trader_id.unwrap_or_default()
    }

    // ----------- StrategyId ----------
    pub fn strategy_id(&mut self, strategy_id: StrategyId) -> &mut Self {
        self.strategy_id = Some(strategy_id);
        self
    }

    fn get_strategy_id(&self) -> StrategyId {
        self.strategy_id.unwrap_or_default()
    }

    // ----------- InstrumentId ----------
    pub fn instrument_id(&mut self, instrument_id: InstrumentId) -> &mut Self {
        self.instrument_id = Some(instrument_id);
        self
    }

    fn get_instrument_id(&self) -> InstrumentId {
        self.instrument_id.expect("Instrument ID not set")
    }

    // ----------- ClientOrderId ----------
    pub fn client_order_id(&mut self, client_order_id: ClientOrderId) -> &mut Self {
        self.client_order_id = Some(client_order_id);
        self
    }

    fn get_client_order_id(&self) -> ClientOrderId {
        self.client_order_id.unwrap_or_default()
    }

    // ----------- TradeId ----------
    pub fn trade_id(&mut self, trade_id: TradeId) -> &mut Self {
        self.trade_id = Some(trade_id);
        self
    }

    fn get_trade_id(&self) -> TradeId {
        self.trade_id.unwrap_or_default()
    }

    // ----------- Currency ----------
    pub fn currency(&mut self, currency: Currency) -> &mut Self {
        self.currency = Some(currency);
        self
    }

    fn get_currency(&self) -> Currency {
        self.currency.unwrap_or(Currency::from("USDT"))
    }

    // ----------- OrderSide ----------
    pub fn side(&mut self, side: OrderSide) -> &mut Self {
        self.side = Some(side);
        self
    }

    fn get_side(&self) -> OrderSide {
        self.side.unwrap_or(OrderSide::Buy)
    }

    // ----------- Quantity ----------
    pub fn quantity(&mut self, quantity: Quantity) -> &mut Self {
        self.quantity = Some(quantity);
        self
    }

    fn get_quantity(&self) -> Quantity {
        self.quantity.expect("Order quantity not set")
    }

    // ----------- Price ----------
    pub fn price(&mut self, price: Price) -> &mut Self {
        self.price = Some(price);
        self
    }

    fn get_price(&self) -> Price {
        self.price.expect("Price not set")
    }

    // ----------- TriggerPrice ----------
    pub fn trigger_price(&mut self, trigger_price: Price) -> &mut Self {
        self.trigger_price = Some(trigger_price);
        self
    }

    fn get_trigger_price(&self) -> Price {
        self.trigger_price.expect("Trigger price not set")
    }

    // ----------- TriggerType ----------
    pub fn trigger_type(&mut self, trigger_type: TriggerType) -> &mut Self {
        self.trigger_type = Some(trigger_type);
        self
    }

    fn get_trigger_type(&self) -> TriggerType {
        self.trigger_type.unwrap_or(TriggerType::Default)
    }

    // ----------- LimitOffset ----------
    pub fn limit_offset(&mut self, limit_offset: Decimal) -> &mut Self {
        self.limit_offset = Some(limit_offset);
        self
    }

    fn get_limit_offset(&self) -> Decimal {
        self.limit_offset.expect("Limit offset not set")
    }

    // ----------- TrailingOffset ----------
    pub fn trailing_offset(&mut self, trailing_offset: Decimal) -> &mut Self {
        self.trailing_offset = Some(trailing_offset);
        self
    }

    fn get_trailing_offset(&self) -> Decimal {
        self.trailing_offset.expect("Trailing offset not set")
    }

    // ----------- TrailingOffsetType ----------
    pub fn trailing_offset_type(&mut self, trailing_offset_type: TrailingOffsetType) -> &mut Self {
        self.trailing_offset_type = Some(trailing_offset_type);
        self
    }

    fn get_trailing_offset_type(&self) -> TrailingOffsetType {
        self.trailing_offset_type
            .unwrap_or(TrailingOffsetType::NoTrailingOffset)
    }

    // ----------- TimeInForce ----------
    pub fn time_in_force(&mut self, time_in_force: TimeInForce) -> &mut Self {
        self.time_in_force = Some(time_in_force);
        self
    }

    fn get_time_in_force(&self) -> TimeInForce {
        self.time_in_force.unwrap_or(TimeInForce::Gtc)
    }

    // ----------- ExpireTime ----------
    pub fn expire_time(&mut self, expire_time: UnixNanos) -> &mut Self {
        self.expire_time = Some(expire_time);
        self
    }

    fn get_expire_time(&self) -> Option<UnixNanos> {
        self.expire_time
    }

    // ----------- DisplayQty ----------
    pub fn display_qty(&mut self, display_qty: Quantity) -> &mut Self {
        self.display_qty = Some(display_qty);
        self
    }

    fn get_display_qty(&self) -> Option<Quantity> {
        self.display_qty
    }

    // ----------- LiquiditySide ----------
    pub fn liquidity_side(&mut self, liquidity_side: LiquiditySide) -> &mut Self {
        self.liquidity_side = Some(liquidity_side);
        self
    }

    fn get_liquidity_side(&self) -> LiquiditySide {
        self.liquidity_side.unwrap_or(LiquiditySide::Maker)
    }

    // ----------- EmulationTrigger ----------
    pub fn emulation_trigger(&mut self, emulation_trigger: TriggerType) -> &mut Self {
        self.emulation_trigger = Some(emulation_trigger);
        self
    }

    fn get_emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    // ----------- TriggerInstrumentId ----------
    pub fn trigger_instrument_id(&mut self, trigger_instrument_id: InstrumentId) -> &mut Self {
        self.trigger_instrument_id = Some(trigger_instrument_id);
        self
    }

    fn get_trigger_instrument_id(&self) -> Option<InstrumentId> {
        self.trigger_instrument_id
    }

    // ----------- OrderListId ----------
    pub fn order_list_id(&mut self, order_list_id: OrderListId) -> &mut Self {
        self.order_list_id = Some(order_list_id);
        self
    }

    fn get_order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    // ----------- LinkedOrderIds ----------
    pub fn linked_order_ids(&mut self, linked_order_ids: Vec<ClientOrderId>) -> &mut Self {
        self.linked_order_ids = Some(linked_order_ids);
        self
    }

    fn get_linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        self.linked_order_ids.clone()
    }

    // ----------- ParentOrderId ----------
    pub fn parent_order_id(&mut self, parent_order_id: ClientOrderId) -> &mut Self {
        self.parent_order_id = Some(parent_order_id);
        self
    }

    fn get_parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    // ----------- ExecAlgorithmId ----------
    pub fn exec_algorithm_id(&mut self, exec_algorithm_id: ExecAlgorithmId) -> &mut Self {
        self.exec_algorithm_id = Some(exec_algorithm_id);
        self
    }

    fn get_exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    // ----------- ExecAlgorithmParams ----------
    pub fn exec_algorithm_params(
        &mut self,
        exec_algorithm_params: IndexMap<Ustr, Ustr>,
    ) -> &mut Self {
        self.exec_algorithm_params = Some(exec_algorithm_params);
        self
    }

    fn get_exec_algorithm_params(&self) -> Option<IndexMap<Ustr, Ustr>> {
        self.exec_algorithm_params.clone()
    }

    // ----------- ExecSpawnId ----------
    pub fn exec_spawn_id(&mut self, exec_spawn_id: ClientOrderId) -> &mut Self {
        self.exec_spawn_id = Some(exec_spawn_id);
        self
    }

    fn get_exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    // ----------- Tags ----------
    pub fn tags(&mut self, tags: Vec<Ustr>) -> &mut Self {
        self.tags = Some(tags);
        self
    }

    fn get_tags(&self) -> Option<Vec<Ustr>> {
        self.tags.clone()
    }

    // ----------- InitId ----------
    pub fn init_id(&mut self, init_id: UUID4) -> &mut Self {
        self.init_id = Some(init_id);
        self
    }

    fn get_init_id(&self) -> UUID4 {
        self.init_id.unwrap_or_default()
    }

    // ----------- TsInit ----------
    pub fn ts_init(&mut self, ts_init: UnixNanos) -> &mut Self {
        self.ts_init = Some(ts_init);
        self
    }

    fn get_ts_init(&self) -> UnixNanos {
        self.ts_init.unwrap_or_default()
    }

    // ----------- ReduceOnly ----------
    pub fn reduce_only(&mut self, reduce_only: bool) -> &mut Self {
        self.reduce_only = Some(reduce_only);
        self
    }

    fn get_reduce_only(&self) -> bool {
        self.reduce_only.unwrap_or(false)
    }

    // ----------- PostOnly ----------
    pub fn post_only(&mut self, post_only: bool) -> &mut Self {
        self.post_only = Some(post_only);
        self
    }

    fn get_post_only(&self) -> bool {
        self.post_only.unwrap_or(false)
    }

    // ----------- QuoteQuantity ----------
    pub fn quote_quantity(&mut self, quote_quantity: bool) -> &mut Self {
        self.quote_quantity = Some(quote_quantity);
        self
    }

    fn get_quote_quantity(&self) -> bool {
        self.quote_quantity.unwrap_or(false)
    }

    // ----------- Reconciliation ----------
    pub fn reconciliation(&mut self, reconciliation: bool) -> &mut Self {
        self.reconciliation = Some(reconciliation);
        self
    }

    fn get_reconciliation(&self) -> bool {
        self.reconciliation.unwrap_or(false)
    }

    // ----------- ContingencyType ----------
    pub fn contingency_type(&mut self, contingency_type: ContingencyType) -> &mut Self {
        self.contingency_type = Some(contingency_type);
        self
    }

    fn get_contingency_type(&self) -> Option<ContingencyType> {
        Some(
            self.contingency_type
                .unwrap_or(ContingencyType::NoContingency),
        )
    }

    /// Builds the order, consuming the provided parameters.
    ///
    /// # Panics
    ///
    /// Panics if required fields (instrument ID, quantity, price, offsets, etc.) are not set,
    /// or if internal calls to `.expect(...)` or `.unwrap()` fail during order construction.
    pub fn build(&self) -> OrderAny {
        let mut order = match self.kind {
            OrderType::Market => OrderAny::Market(MarketOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_time_in_force(),
                self.get_init_id(),
                self.get_ts_init(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
            )),
            OrderType::Limit => OrderAny::Limit(LimitOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_price(),
                self.get_time_in_force(),
                self.get_expire_time(),
                self.get_post_only(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_display_qty(),
                self.get_emulation_trigger(),
                self.get_trigger_instrument_id(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
                self.get_init_id(),
                self.get_ts_init(),
            )),
            OrderType::StopMarket => OrderAny::StopMarket(StopMarketOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_trigger_price(),
                self.get_trigger_type(),
                self.get_time_in_force(),
                self.get_expire_time(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_display_qty(),
                self.get_emulation_trigger(),
                self.get_trigger_instrument_id(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
                self.get_init_id(),
                self.get_ts_init(),
            )),
            OrderType::StopLimit => OrderAny::StopLimit(StopLimitOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_price(),
                self.get_trigger_price(),
                self.get_trigger_type(),
                self.get_time_in_force(),
                self.get_expire_time(),
                self.get_post_only(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_display_qty(),
                self.get_emulation_trigger(),
                self.get_trigger_instrument_id(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
                self.get_init_id(),
                self.get_ts_init(),
            )),
            OrderType::MarketToLimit => OrderAny::MarketToLimit(MarketToLimitOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_time_in_force(),
                self.get_expire_time(),
                self.get_post_only(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_display_qty(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
                self.get_init_id(),
                self.get_ts_init(),
            )),
            OrderType::MarketIfTouched => OrderAny::MarketIfTouched(MarketIfTouchedOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_trigger_price(),
                self.get_trigger_type(),
                self.get_time_in_force(),
                self.get_expire_time(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_emulation_trigger(),
                self.get_trigger_instrument_id(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
                self.get_init_id(),
                self.get_ts_init(),
            )),
            OrderType::LimitIfTouched => OrderAny::LimitIfTouched(LimitIfTouchedOrder::new(
                self.get_trader_id(),
                self.get_strategy_id(),
                self.get_instrument_id(),
                self.get_client_order_id(),
                self.get_side(),
                self.get_quantity(),
                self.get_price(),
                self.get_trigger_price(),
                self.get_trigger_type(),
                self.get_time_in_force(),
                self.get_expire_time(),
                self.get_post_only(),
                self.get_reduce_only(),
                self.get_quote_quantity(),
                self.get_display_qty(),
                self.get_emulation_trigger(),
                self.get_trigger_instrument_id(),
                self.get_contingency_type(),
                self.get_order_list_id(),
                self.get_linked_order_ids(),
                self.get_parent_order_id(),
                self.get_exec_algorithm_id(),
                self.get_exec_algorithm_params(),
                self.get_exec_spawn_id(),
                self.get_tags(),
                self.get_init_id(),
                self.get_ts_init(),
            )),
            OrderType::TrailingStopMarket => {
                OrderAny::TrailingStopMarket(TrailingStopMarketOrder::new(
                    self.get_trader_id(),
                    self.get_strategy_id(),
                    self.get_instrument_id(),
                    self.get_client_order_id(),
                    self.get_side(),
                    self.get_quantity(),
                    self.get_trigger_price(),
                    self.get_trigger_type(),
                    self.get_trailing_offset(),
                    self.get_trailing_offset_type(),
                    self.get_time_in_force(),
                    self.get_expire_time(),
                    self.get_reduce_only(),
                    self.get_quote_quantity(),
                    self.get_display_qty(),
                    self.get_emulation_trigger(),
                    self.get_trigger_instrument_id(),
                    self.get_contingency_type(),
                    self.get_order_list_id(),
                    self.get_linked_order_ids(),
                    self.get_parent_order_id(),
                    self.get_exec_algorithm_id(),
                    self.get_exec_algorithm_params(),
                    self.get_exec_spawn_id(),
                    self.get_tags(),
                    self.get_init_id(),
                    self.get_ts_init(),
                ))
            }
            OrderType::TrailingStopLimit => {
                OrderAny::TrailingStopLimit(TrailingStopLimitOrder::new(
                    self.get_trader_id(),
                    self.get_strategy_id(),
                    self.get_instrument_id(),
                    self.get_client_order_id(),
                    self.get_side(),
                    self.get_quantity(),
                    self.get_price(),
                    self.get_trigger_price(),
                    self.get_trigger_type(),
                    self.get_limit_offset(),
                    self.get_trailing_offset(),
                    self.get_trailing_offset_type(),
                    self.get_time_in_force(),
                    self.get_expire_time(),
                    self.get_post_only(),
                    self.get_reduce_only(),
                    self.get_quote_quantity(),
                    self.get_display_qty(),
                    self.get_emulation_trigger(),
                    self.get_trigger_instrument_id(),
                    self.get_contingency_type(),
                    self.get_order_list_id(),
                    self.get_linked_order_ids(),
                    self.get_parent_order_id(),
                    self.get_exec_algorithm_id(),
                    self.get_exec_algorithm_params(),
                    self.get_exec_spawn_id(),
                    self.get_tags(),
                    self.get_init_id(),
                    self.get_ts_init(),
                ))
            }
        };

        if self.submitted {
            let submit_event = OrderSubmitted::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                AccountId::from("ACCOUNT-001"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            );
            order.apply(OrderEventAny::Submitted(submit_event)).unwrap();
        }

        order
    }
}

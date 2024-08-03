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

//! An `OrderMatchingEngine` for use in research, backtesting and sandbox environments.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{any::Any, collections::HashMap, rc::Rc};

use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{nanos::UnixNanos, time::AtomicTime, uuid::UUID4};
use nautilus_execution::matching_core::OrderMatchingCore;
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        delta::OrderBookDelta,
    },
    enums::{AccountType, BookType, LiquiditySide, MarketStatus, OmsType},
    events::order::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderExpired,
        OrderFilled, OrderModifyRejected, OrderRejected, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{any::InstrumentAny, EXPIRING_INSTRUMENT_TYPES},
    orderbook::book::OrderBook,
    orders::{
        any::{OrderAny, PassiveOrderAny, StopOrderAny},
        trailing_stop_limit::TrailingStopLimitOrder,
        trailing_stop_market::TrailingStopMarketOrder,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};
use ustr::Ustr;

#[derive(Debug, Clone)]
pub struct OrderMatchingEngineConfig {
    pub bar_execution: bool,
    pub reject_stop_orders: bool,
    pub support_gtd_orders: bool,
    pub support_contingent_orders: bool,
    pub use_position_ids: bool,
    pub use_random_ids: bool,
    pub use_reduce_only: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for OrderMatchingEngineConfig {
    fn default() -> Self {
        OrderMatchingEngineConfig {
            bar_execution: false,
            reject_stop_orders: false,
            support_gtd_orders: false,
            support_contingent_orders: false,
            use_position_ids: false,
            use_random_ids: false,
            use_reduce_only: false,
        }
    }
}

/// An order matching engine for a single market.
pub struct OrderMatchingEngine {
    /// The venue for the matching engine.
    pub venue: Venue,
    /// The instrument for the matching engine.
    pub instrument: InstrumentAny,
    /// The instruments raw integer ID for the venue.
    pub raw_id: u32,
    /// The order book type for the matching engine.
    pub book_type: BookType,
    /// The order management system (OMS) type for the matching engine.
    pub oms_type: OmsType,
    /// The account type for the matching engine.
    pub account_type: AccountType,
    /// The market status for the matching engine.
    pub market_status: MarketStatus,
    /// The config for the matching engine.
    pub config: OrderMatchingEngineConfig,
    clock: &'static AtomicTime,
    msgbus: Rc<MessageBus>,
    cache: Rc<Cache>,
    book: OrderBook,
    core: OrderMatchingCore,
    target_bid: Option<Price>,
    target_ask: Option<Price>,
    target_last: Option<Price>,
    last_bar_bid: Option<Bar>,
    last_bar_ask: Option<Bar>,
    execution_bar_types: HashMap<InstrumentId, BarType>,
    execution_bar_deltas: HashMap<InstrumentId, u64>,
    account_ids: HashMap<TraderId, AccountId>,
    position_count: usize,
    order_count: usize,
    execution_count: usize,
}

// TODO: we'll probably be changing the `FillModel` (don't add for now)
impl OrderMatchingEngine {
    /// Creates a new [`OrderMatchingEngine`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: InstrumentAny,
        raw_id: u32,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        clock: &'static AtomicTime,
        msgbus: Rc<MessageBus>,
        cache: Rc<Cache>,
        config: OrderMatchingEngineConfig,
    ) -> Self {
        let book = OrderBook::new(book_type, instrument.id());
        let core = OrderMatchingCore::new(
            instrument.id(),
            instrument.price_increment(),
            None, // TBD (will be a function on the engine)
            None, // TBD (will be a function on the engine)
            None, // TBD (will be a function on the engine)
        );
        Self {
            venue: instrument.id().venue,
            instrument,
            raw_id,
            book_type,
            oms_type,
            account_type,
            clock,
            msgbus,
            cache,
            book,
            core,
            market_status: MarketStatus::Open,
            config,
            target_bid: None,
            target_ask: None,
            target_last: None,
            last_bar_bid: None,
            last_bar_ask: None,
            execution_bar_types: HashMap::new(),
            execution_bar_deltas: HashMap::new(),
            account_ids: HashMap::new(),
            position_count: 0,
            order_count: 0,
            execution_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.book.clear(0, UnixNanos::default());
        self.execution_bar_types.clear();
        self.execution_bar_deltas.clear();
        self.account_ids.clear();
        self.core.reset();
        self.target_bid = None;
        self.target_ask = None;
        self.target_last = None;
        self.position_count = 0;
        self.order_count = 0;
        self.execution_count = 0;

        log::info!("Reset {}", self.instrument.id());
    }

    #[must_use]
    pub fn best_bid_price(&self) -> Option<Price> {
        self.book.best_bid_price()
    }

    #[must_use]
    pub fn best_ask_price(&self) -> Option<Price> {
        self.book.best_ask_price()
    }

    #[must_use]
    pub const fn get_book(&self) -> &OrderBook {
        &self.book
    }

    #[must_use]
    pub fn get_open_bid_orders(&self) -> &[PassiveOrderAny] {
        self.core.get_orders_bid()
    }

    #[must_use]
    pub fn get_open_ask_orders(&self) -> &[PassiveOrderAny] {
        self.core.get_orders_ask()
    }

    #[must_use]
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.core.order_exists(client_order_id)
    }

    // -- DATA PROCESSING -----------------------------------------------------

    /// Process the venues market for the given order book delta.
    pub fn process_order_book_delta(&mut self, delta: &OrderBookDelta) {
        log::debug!("Processing {delta}");

        self.book.apply_delta(delta);
    }

    // -- TRADING COMMANDS ----------------------------------------------------
    pub fn process_order(&mut self, order: &OrderAny, account_id: AccountId) {
        if self.core.order_exists(order.client_order_id()) {
            self.generate_order_rejected(order, "Order already exists".into());
            return;
        }

        // Index identifiers
        self.account_ids.insert(order.trader_id(), account_id);

        // Check for instrument expiration or activation
        if EXPIRING_INSTRUMENT_TYPES.contains(&self.instrument.instrument_class()) {
            if let Some(activation_ns) = self.instrument.activation_ns() {
                if self.clock.get_time_ns() < activation_ns {
                    self.generate_order_rejected(
                        order,
                        format!(
                            "Contract {} is not yet active, activation {}",
                            self.instrument.id(),
                            self.instrument.activation_ns().unwrap()
                        )
                        .into(),
                    );
                }
            }
            if let Some(expiration_ns) = self.instrument.expiration_ns() {
                if self.clock.get_time_ns() >= expiration_ns {
                    self.generate_order_rejected(
                        order,
                        format!(
                            "Contract {} has expired, expiration {}",
                            self.instrument.id(),
                            self.instrument.expiration_ns().unwrap()
                        )
                        .into(),
                    );
                }
            }
        }
    }

    // -- ORDER PROCESSING ----------------------------------------------------

    /// Iterate the matching engine by processing the bid and ask order sides
    /// and advancing time up to the given UNIX `timestamp_ns`.
    pub fn iterate(&mut self, timestamp_ns: UnixNanos) {
        self.clock.set_time(timestamp_ns);

        self.core.bid = self.book.best_bid_price();
        self.core.ask = self.book.best_ask_price();

        let orders_bid = self.core.get_orders_bid().to_vec();
        let orders_ask = self.core.get_orders_ask().to_vec();

        self.iterate_orders(timestamp_ns, &orders_bid);
        self.iterate_orders(timestamp_ns, &orders_ask);
    }

    fn iterate_orders(&mut self, timestamp_ns: UnixNanos, orders: &[PassiveOrderAny]) {
        for order in orders {
            if order.is_closed() {
                continue;
            };

            // Check expiration
            if self.config.support_gtd_orders {
                if let Some(expire_time) = order.expire_time() {
                    if timestamp_ns >= expire_time {
                        // SAFTEY: We know this order is in the core
                        self.core.delete_order(order).unwrap();
                        self.expire_order(order);
                    }
                }
            }

            // Manage trailing stop
            if let PassiveOrderAny::Stop(o) = order {
                match o {
                    StopOrderAny::TrailingStopMarket(o) => self.update_trailing_stop_market(o),
                    StopOrderAny::TrailingStopLimit(o) => self.update_trailing_stop_limit(o),
                    _ => {}
                }
            }

            // Move market back to targets
            self.core.bid = self.target_bid;
            self.core.ask = self.target_ask;
            self.core.last = self.target_last;
        }

        // Reset any targets after iteration
        self.target_bid = None;
        self.target_ask = None;
        self.target_last = None;
    }

    fn expire_order(&mut self, order: &PassiveOrderAny) {
        todo!();
    }

    fn update_trailing_stop_market(&mut self, order: &TrailingStopMarketOrder) {
        todo!()
    }

    fn update_trailing_stop_limit(&mut self, order: &TrailingStopLimitOrder) {
        todo!()
    }

    // -- IDENTIFIER GENERATORS -----------------------------------------------------

    fn generate_trade_id(&mut self) -> TradeId {
        self.execution_count += 1;
        TradeId::new(self.generate_trade_id_str().as_str()).unwrap()
    }

    fn generate_trade_id_str(&self) -> Ustr {
        if self.config.use_random_ids {
            UUID4::new().to_string().into()
        } else {
            format!("{}-{}-{}", self.venue, self.raw_id, self.execution_count).into()
        }
    }

    // -- EVENT GENERATORS -----------------------------------------------------

    fn generate_order_rejected(&self, order: &OrderAny, reason: Ustr) {
        let ts_now = self.clock.get_time_ns();
        let account_id = order
            .account_id()
            .unwrap_or(self.account_ids.get(&order.trader_id()).unwrap().to_owned());

        let event = OrderEventAny::Rejected(
            OrderRejected::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                account_id,
                reason,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    fn generate_order_accepted(&self, order: &OrderAny, venue_order_id: VenueOrderId) {
        let ts_now = self.clock.get_time_ns();
        let account_id = order
            .account_id()
            .unwrap_or(self.account_ids.get(&order.trader_id()).unwrap().to_owned());
        let event = OrderEventAny::Accepted(
            OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                venue_order_id,
                account_id,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_modify_rejected(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: Ustr,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::ModifyRejected(
            OrderModifyRejected::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                reason,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                Some(venue_order_id),
                Some(account_id),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_cancel_rejected(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: Ustr,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::CancelRejected(
            OrderCancelRejected::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                reason,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                Some(venue_order_id),
                Some(account_id),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    fn generate_order_updated(
        &self,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Updated(
            OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                quantity,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                order.venue_order_id(),
                order.account_id(),
                Some(price),
                Some(trigger_price),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    fn generate_order_canceled(&self, order: &OrderAny, venue_order_id: VenueOrderId) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Canceled(
            OrderCanceled::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                Some(venue_order_id),
                order.account_id(),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    fn generate_order_triggered(&self, order: &OrderAny) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Triggered(
            OrderTriggered::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                order.venue_order_id(),
                order.account_id(),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    fn generate_order_expired(&self, order: &OrderAny) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Expired(
            OrderExpired::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                order.venue_order_id(),
                order.account_id(),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_filled(
        &mut self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        venue_position_id: PositionId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
    ) {
        let ts_now = self.clock.get_time_ns();
        let account_id = order
            .account_id()
            .unwrap_or(self.account_ids.get(&order.trader_id()).unwrap().to_owned());
        let event = OrderEventAny::Filled(
            OrderFilled::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                venue_order_id,
                account_id,
                self.generate_trade_id(),
                order.order_side(),
                order.order_type(),
                last_qty,
                last_px,
                quote_currency,
                liquidity_side,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                Some(venue_position_id),
                Some(commission),
            )
            .unwrap(),
        );
        self.msgbus.send("ExecEngine.process", &event as &dyn Any);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{rc::Rc, sync::LazyLock};

    use chrono::{TimeZone, Utc};
    use nautilus_common::{
        cache::Cache,
        msgbus::{
            stubs::{get_message_saving_handler, MessageSavingHandler},
            MessageBus,
        },
    };
    use nautilus_core::{nanos::UnixNanos, time::AtomicTime};
    use nautilus_model::{
        enums::{AccountType, BookType, OmsType, OrderSide},
        events::order::{OrderEventAny, OrderEventType},
        identifiers::AccountId,
        instruments::{any::InstrumentAny, stubs::futures_contract_es},
        orders::stubs::TestOrderStubs,
        types::quantity::Quantity,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use crate::matching_engine::{OrderMatchingEngine, OrderMatchingEngineConfig};

    static ATOMIC_TIME: LazyLock<AtomicTime> =
        LazyLock::new(|| AtomicTime::new(true, UnixNanos::default()));

    fn get_order_matching_engine(
        instrument: InstrumentAny,
        msgbus: Rc<MessageBus>,
    ) -> OrderMatchingEngine {
        let cache = Rc::new(Cache::default());
        let config = OrderMatchingEngineConfig::default();
        OrderMatchingEngine::new(
            instrument,
            1,
            BookType::L1_MBP,
            OmsType::Netting,
            AccountType::Cash,
            &ATOMIC_TIME,
            msgbus,
            cache,
            config,
        )
    }

    #[rstest]
    fn test_order_matching_engine_instrument_already_expired() {
        let account_id = AccountId::from("SIM-001");
        let time = AtomicTime::new(true, UnixNanos::default());
        let mut msgbus = MessageBus::default();
        let instrument = InstrumentAny::FuturesContract(futures_contract_es(None, None));

        // Register saving message handler to exec engine endpoint
        let exec_engine_endpoint = "ExecEngine.process";
        let msg_handler =
            get_message_saving_handler::<OrderEventAny>(Ustr::from(exec_engine_endpoint));
        msgbus.register(exec_engine_endpoint, msg_handler.clone());

        // Create engine and process order
        let mut engine = get_order_matching_engine(instrument.clone(), Rc::new(msgbus));
        let order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
        );
        engine.process_order(&order, account_id);

        // Get messages and test
        let saved_messages = msg_handler
            .0
            .as_ref()
            .as_any()
            .downcast_ref::<MessageSavingHandler<OrderEventAny>>()
            .unwrap()
            .get_messages();
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Contract ESZ1.GLBX has expired, expiration 1625702400000000000")
        );
    }

    #[rstest]
    fn test_order_matching_engine_instrument_not_active() {
        let account_id = AccountId::from("SIM-001");
        let time = AtomicTime::new(true, UnixNanos::default());
        let mut msgbus = MessageBus::default();
        let activation = UnixNanos::from(
            Utc.with_ymd_and_hms(2222, 4, 8, 0, 0, 0)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64,
        );
        let expiration = UnixNanos::from(
            Utc.with_ymd_and_hms(2223, 7, 8, 0, 0, 0)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64,
        );
        let instrument =
            InstrumentAny::FuturesContract(futures_contract_es(Some(activation), Some(expiration)));

        // Register saving message handler to exec engine endpoint
        let exec_engine_endpoint = "ExecEngine.process";
        let msg_handler =
            get_message_saving_handler::<OrderEventAny>(Ustr::from(exec_engine_endpoint));
        msgbus.register(exec_engine_endpoint, msg_handler.clone());

        // Create engine and process order
        let mut engine = get_order_matching_engine(instrument.clone(), Rc::new(msgbus));
        let order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
        );
        engine.process_order(&order, account_id);

        // Get messages and test
        let saved_messages = msg_handler
            .0
            .as_ref()
            .as_any()
            .downcast_ref::<MessageSavingHandler<OrderEventAny>>()
            .unwrap()
            .get_messages();
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Contract ESZ1.GLBX is not yet active, activation 7960723200000000000")
        );
    }
}

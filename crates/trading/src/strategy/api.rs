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

//! User-facing strategy APIs.

use std::cell::RefCell;

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_analysis::snapshot::PortfolioStatistics;
use nautilus_common::factories::OrderFactory;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    events::PortfolioSnapshot,
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId, Venue,
    },
    orders::{OrderAny, OrderList},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use rust_decimal::Decimal;
use ustr::Ustr;

/// User-facing order creation API.
#[derive(Debug)]
pub struct OrderApi<'a> {
    order_factory: &'a RefCell<OrderFactory>,
}

#[bon::bon]
impl<'a> OrderApi<'a> {
    pub(crate) const fn new(order_factory: &'a RefCell<OrderFactory>) -> Self {
        Self { order_factory }
    }

    /// Generates a new client order ID.
    ///
    /// # Panics
    ///
    /// Panics if the order factory is already mutably borrowed.
    #[must_use]
    pub fn generate_client_order_id(&self) -> ClientOrderId {
        self.order_factory.borrow_mut().generate_client_order_id()
    }

    /// Generates a new order list ID.
    ///
    /// # Panics
    ///
    /// Panics if the order factory is already mutably borrowed.
    #[must_use]
    pub fn generate_order_list_id(&self) -> OrderListId {
        self.order_factory.borrow_mut().generate_order_list_id()
    }

    /// Creates a new market order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn market(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().market(
            instrument_id,
            order_side,
            quantity,
            time_in_force,
            reduce_only,
            quote_quantity,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new limit order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn limit(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().limit(
            instrument_id,
            order_side,
            quantity,
            price,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new stop-market order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn stop_market(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().stop_market(
            instrument_id,
            order_side,
            quantity,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new stop-limit order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn stop_limit(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().stop_limit(
            instrument_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new market-to-limit order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn market_to_limit(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().market_to_limit(
            instrument_id,
            order_side,
            quantity,
            time_in_force,
            expire_time,
            reduce_only,
            quote_quantity,
            display_qty,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new market-if-touched order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn market_if_touched(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().market_if_touched(
            instrument_id,
            order_side,
            quantity,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            reduce_only,
            quote_quantity,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new limit-if-touched order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn limit_if_touched(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().limit_if_touched(
            instrument_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new trailing-stop-market order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn trailing_stop_market(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trailing_offset: Decimal,
        trailing_offset_type: Option<TrailingOffsetType>,
        activation_price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().trailing_stop_market(
            instrument_id,
            order_side,
            quantity,
            trailing_offset,
            trailing_offset_type,
            activation_price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new trailing-stop-limit order.
    ///
    /// # Panics
    ///
    /// Panics if the order parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn trailing_stop_limit(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        trailing_offset_type: Option<TrailingOffsetType>,
        activation_price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        self.order_factory.borrow_mut().trailing_stop_limit(
            instrument_id,
            order_side,
            quantity,
            price,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            activation_price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tags,
            client_order_id,
        )
    }

    /// Creates a new order list from the given orders.
    ///
    /// # Panics
    ///
    /// Panics if the list parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    pub fn create_list(&self, orders: &mut [OrderAny], ts_init: UnixNanos) -> OrderList {
        self.order_factory.borrow_mut().create_list(orders, ts_init)
    }

    /// Creates a bracket order with an entry order and attached take-profit and stop-loss legs.
    ///
    /// # Panics
    ///
    /// Panics if the bracket parameters fail validation or the order factory is already mutably
    /// borrowed.
    #[must_use]
    #[builder]
    pub fn bracket(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        #[builder(default = false)] quote_quantity: bool,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        #[builder(default = ContingencyType::Ouo)] contingency_type: ContingencyType,
        #[builder(default = OrderType::Market)] entry_order_type: OrderType,
        entry_price: Option<Price>,
        entry_trigger_price: Option<Price>,
        expire_time: Option<UnixNanos>,
        #[builder(default = TimeInForce::Gtc)] time_in_force: TimeInForce,
        #[builder(default = false)] entry_post_only: bool,
        entry_exec_algorithm_id: Option<ExecAlgorithmId>,
        entry_exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        #[builder(default = vec![Ustr::from("ENTRY")])] entry_tags: Vec<Ustr>,
        entry_client_order_id: Option<ClientOrderId>,
        #[builder(default = OrderType::Limit)] tp_order_type: OrderType,
        tp_price: Option<Price>,
        tp_trigger_price: Option<Price>,
        #[builder(default = TriggerType::Default)] tp_trigger_type: TriggerType,
        tp_activation_price: Option<Price>,
        tp_trailing_offset: Option<Decimal>,
        #[builder(default = TrailingOffsetType::Price)] tp_trailing_offset_type: TrailingOffsetType,
        tp_limit_offset: Option<Decimal>,
        #[builder(default = TimeInForce::Gtc)] tp_time_in_force: TimeInForce,
        #[builder(default = true)] tp_post_only: bool,
        tp_exec_algorithm_id: Option<ExecAlgorithmId>,
        tp_exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        #[builder(default = vec![Ustr::from("TAKE_PROFIT")])] tp_tags: Vec<Ustr>,
        tp_client_order_id: Option<ClientOrderId>,
        #[builder(default = OrderType::StopMarket)] sl_order_type: OrderType,
        sl_trigger_price: Option<Price>,
        #[builder(default = TriggerType::Default)] sl_trigger_type: TriggerType,
        sl_activation_price: Option<Price>,
        sl_trailing_offset: Option<Decimal>,
        #[builder(default = TrailingOffsetType::Price)] sl_trailing_offset_type: TrailingOffsetType,
        #[builder(default = TimeInForce::Gtc)] sl_time_in_force: TimeInForce,
        sl_exec_algorithm_id: Option<ExecAlgorithmId>,
        sl_exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        #[builder(default = vec![Ustr::from("STOP_LOSS")])] sl_tags: Vec<Ustr>,
        sl_client_order_id: Option<ClientOrderId>,
    ) -> Vec<OrderAny> {
        let mut order_factory = self.order_factory.borrow_mut();
        order_factory
            .bracket()
            .instrument_id(instrument_id)
            .order_side(order_side)
            .quantity(quantity)
            .quote_quantity(quote_quantity)
            .maybe_emulation_trigger(emulation_trigger)
            .maybe_trigger_instrument_id(trigger_instrument_id)
            .contingency_type(contingency_type)
            .entry_order_type(entry_order_type)
            .maybe_entry_price(entry_price)
            .maybe_entry_trigger_price(entry_trigger_price)
            .maybe_expire_time(expire_time)
            .time_in_force(time_in_force)
            .entry_post_only(entry_post_only)
            .maybe_entry_exec_algorithm_id(entry_exec_algorithm_id)
            .maybe_entry_exec_algorithm_params(entry_exec_algorithm_params)
            .entry_tags(entry_tags)
            .maybe_entry_client_order_id(entry_client_order_id)
            .tp_order_type(tp_order_type)
            .maybe_tp_price(tp_price)
            .maybe_tp_trigger_price(tp_trigger_price)
            .tp_trigger_type(tp_trigger_type)
            .maybe_tp_activation_price(tp_activation_price)
            .maybe_tp_trailing_offset(tp_trailing_offset)
            .tp_trailing_offset_type(tp_trailing_offset_type)
            .maybe_tp_limit_offset(tp_limit_offset)
            .tp_time_in_force(tp_time_in_force)
            .tp_post_only(tp_post_only)
            .maybe_tp_exec_algorithm_id(tp_exec_algorithm_id)
            .maybe_tp_exec_algorithm_params(tp_exec_algorithm_params)
            .tp_tags(tp_tags)
            .maybe_tp_client_order_id(tp_client_order_id)
            .sl_order_type(sl_order_type)
            .maybe_sl_trigger_price(sl_trigger_price)
            .sl_trigger_type(sl_trigger_type)
            .maybe_sl_activation_price(sl_activation_price)
            .maybe_sl_trailing_offset(sl_trailing_offset)
            .sl_trailing_offset_type(sl_trailing_offset_type)
            .sl_time_in_force(sl_time_in_force)
            .maybe_sl_exec_algorithm_id(sl_exec_algorithm_id)
            .maybe_sl_exec_algorithm_params(sl_exec_algorithm_params)
            .sl_tags(sl_tags)
            .maybe_sl_client_order_id(sl_client_order_id)
            .call()
    }
}

/// User-facing portfolio read API.
#[derive(Debug)]
pub struct PortfolioApi<'a> {
    portfolio: &'a RefCell<Portfolio>,
}

impl<'a> PortfolioApi<'a> {
    pub(crate) const fn new(portfolio: &'a RefCell<Portfolio>) -> Self {
        Self { portfolio }
    }

    /// Returns `true` if the portfolio has been initialized.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.portfolio.borrow().is_initialized()
    }

    /// Returns the locked balances for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn balances_locked(&self, venue: &Venue) -> IndexMap<Currency, Money> {
        self.portfolio.borrow().balances_locked(venue)
    }

    /// Returns the initial margin requirements for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn margins_init(&self, venue: &Venue) -> IndexMap<InstrumentId, Money> {
        self.portfolio.borrow().margins_init(venue)
    }

    /// Returns the maintenance margin requirements for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn margins_maint(&self, venue: &Venue) -> IndexMap<InstrumentId, Money> {
        self.portfolio.borrow().margins_maint(venue)
    }

    /// Returns the unrealized PnLs for all positions at the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn unrealized_pnls(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        self.portfolio
            .borrow_mut()
            .unrealized_pnls(venue, account_id)
    }

    /// Returns the realized PnLs for all positions at the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn realized_pnls(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        self.portfolio.borrow_mut().realized_pnls(venue, account_id)
    }

    /// Returns net exposures by currency for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn net_exposures(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> Option<IndexMap<Currency, Money>> {
        self.portfolio.borrow().net_exposures(venue, account_id)
    }

    /// Returns the unrealized PnL for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn unrealized_pnl(&self, instrument_id: &InstrumentId) -> Option<Money> {
        self.portfolio.borrow_mut().unrealized_pnl(instrument_id)
    }

    /// Returns the unrealized PnL for the given instrument ID and account filter.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn unrealized_pnl_for_account(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        self.portfolio
            .borrow_mut()
            .unrealized_pnl_for_account(instrument_id, account_id)
    }

    /// Returns the realized PnL for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn realized_pnl(&self, instrument_id: &InstrumentId) -> Option<Money> {
        self.portfolio.borrow_mut().realized_pnl(instrument_id)
    }

    /// Returns the realized PnL for the given instrument ID and account filter.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn realized_pnl_for_account(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        self.portfolio
            .borrow_mut()
            .realized_pnl_for_account(instrument_id, account_id)
    }

    /// Returns the total PnL for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn total_pnl(&self, instrument_id: &InstrumentId) -> Option<Money> {
        self.portfolio.borrow_mut().total_pnl(instrument_id)
    }

    /// Returns the total PnL for the given instrument ID and account filter.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn total_pnl_for_account(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        self.portfolio
            .borrow_mut()
            .total_pnl_for_account(instrument_id, account_id)
    }

    /// Returns the total PnLs for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn total_pnls(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        self.portfolio.borrow_mut().total_pnls(venue, account_id)
    }

    /// Returns the per-currency mark-to-market value of open positions at the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn mark_values(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        self.portfolio.borrow_mut().mark_values(venue, account_id)
    }

    /// Returns the per-currency total equity for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn equity(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        self.portfolio.borrow_mut().equity(venue, account_id)
    }

    /// Builds a portfolio snapshot for the given account.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already borrowed.
    #[must_use]
    pub fn build_snapshot(&self, account_id: &AccountId) -> Option<PortfolioSnapshot> {
        self.portfolio.borrow_mut().build_snapshot(account_id)
    }

    /// Returns an owned snapshot of computed portfolio performance statistics.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn statistics(&self) -> PortfolioStatistics {
        self.portfolio.borrow().statistics()
    }

    /// Returns the recorded portfolio snapshots for the given account.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn snapshots(&self, account_id: &AccountId) -> Vec<PortfolioSnapshot> {
        self.portfolio.borrow().snapshots(account_id)
    }

    /// Returns the instruments currently flagged as unpriced for the given venue.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn missing_price_instruments(&self, venue: &Venue) -> Vec<InstrumentId> {
        self.portfolio.borrow().missing_price_instruments(venue)
    }

    /// Returns the net exposure for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn net_exposure(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        self.portfolio
            .borrow()
            .net_exposure(instrument_id, account_id)
    }

    /// Returns the net position for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn net_position(&self, instrument_id: &InstrumentId) -> Decimal {
        self.portfolio.borrow().net_position(instrument_id)
    }

    /// Returns whether the net position is long for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn is_net_long(&self, instrument_id: &InstrumentId) -> bool {
        self.portfolio.borrow().is_net_long(instrument_id)
    }

    /// Returns whether the net position is short for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn is_net_short(&self, instrument_id: &InstrumentId) -> bool {
        self.portfolio.borrow().is_net_short(instrument_id)
    }

    /// Returns whether the net position is flat for the given instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn is_flat(&self, instrument_id: &InstrumentId) -> bool {
        self.portfolio.borrow().is_flat(instrument_id)
    }

    /// Returns whether every net position is flat.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn is_completely_flat(&self) -> bool {
        self.portfolio.borrow().is_completely_flat()
    }

    /// Returns realized PnLs recorded during portfolio event processing.
    ///
    /// Each record is `(position_id, ts_event, realized_pnl)`.
    ///
    /// # Panics
    ///
    /// Panics if the portfolio is already mutably borrowed.
    #[must_use]
    pub fn recorded_realized_pnls(&self) -> AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>> {
        self.portfolio.borrow().recorded_realized_pnls()
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock, factories::OrderFactory};
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{AccountId, InstrumentId, StrategyId, TraderId, Venue},
        orders::Order,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_order_api_creates_market_order() {
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("S-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let order_factory = RefCell::new(OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock,
            false,
            true,
        ));
        let api = OrderApi::new(&order_factory);
        let instrument_id = InstrumentId::from("AUD/USD.SIM");

        let order = api.market(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("100000"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(order.order_type(), OrderType::Market);
        assert_eq!(order.instrument_id(), instrument_id);
        assert_eq!(order.order_side(), OrderSide::Buy);
        assert_eq!(order.quantity(), Quantity::from("100000"));
        assert_eq!(order.trader_id(), trader_id);
        assert_eq!(order.strategy_id(), strategy_id);
    }

    #[rstest]
    fn test_order_api_creates_bracket_orders() {
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("S-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let order_factory = RefCell::new(OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock,
            false,
            true,
        ));
        let api = OrderApi::new(&order_factory);
        let instrument_id = InstrumentId::from("AUD/USD.SIM");

        let orders = api
            .bracket()
            .instrument_id(instrument_id)
            .order_side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .tp_price(Price::from("1.10000"))
            .sl_trigger_price(Price::from("0.90000"))
            .call();

        assert_eq!(orders.len(), 3);
        assert!(
            orders
                .iter()
                .all(|order| order.instrument_id() == instrument_id)
        );
        assert!(orders.iter().all(|order| order.trader_id() == trader_id));
        assert!(
            orders
                .iter()
                .all(|order| order.strategy_id() == strategy_id)
        );
        assert_eq!(orders[0].order_type(), OrderType::Market);
        assert_eq!(orders[0].order_side(), OrderSide::Buy);
        assert_eq!(orders[0].quantity(), Quantity::from("100000"));
        assert_eq!(orders[1].order_type(), OrderType::StopMarket);
        assert_eq!(orders[1].order_side(), OrderSide::Sell);
        assert_eq!(orders[1].trigger_price(), Some(Price::from("0.90000")));
        assert_eq!(orders[2].order_type(), OrderType::Limit);
        assert_eq!(orders[2].order_side(), OrderSide::Sell);
        assert_eq!(orders[2].price(), Some(Price::from("1.10000")));
    }

    #[rstest]
    fn test_portfolio_api_empty_reads_return_empty_values() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let portfolio = RefCell::new(Portfolio::new(cache, clock, None));
        let api = PortfolioApi::new(&portfolio);
        let venue = Venue::from("SIM");
        let account_id = AccountId::from("SIM-001");
        let instrument_id = InstrumentId::from("AUD/USD.SIM");

        assert!(!api.is_initialized());
        assert!(api.balances_locked(&venue).is_empty());
        assert!(api.margins_init(&venue).is_empty());
        assert!(api.margins_maint(&venue).is_empty());
        assert!(api.unrealized_pnls(&venue, None).is_empty());
        assert!(api.realized_pnls(&venue, None).is_empty());
        assert_eq!(api.net_exposures(&venue, None), None);
        assert_eq!(api.unrealized_pnl(&instrument_id), None);
        assert_eq!(
            api.unrealized_pnl_for_account(&instrument_id, Some(&account_id)),
            None
        );
        assert_eq!(api.realized_pnl(&instrument_id), None);
        assert_eq!(
            api.realized_pnl_for_account(&instrument_id, Some(&account_id)),
            None
        );
        assert_eq!(api.total_pnl(&instrument_id), None);
        assert_eq!(
            api.total_pnl_for_account(&instrument_id, Some(&account_id)),
            None
        );
        assert!(api.total_pnls(&venue, None).is_empty());
        assert!(api.mark_values(&venue, None).is_empty());
        assert!(api.equity(&venue, None).is_empty());
        assert_eq!(api.build_snapshot(&account_id), None);
        assert!(api.snapshots(&account_id).is_empty());
        assert!(api.missing_price_instruments(&venue).is_empty());
        assert_eq!(api.net_exposure(&instrument_id, None), None);
        assert_eq!(api.net_position(&instrument_id), Decimal::ZERO);
        assert!(!api.is_net_long(&instrument_id));
        assert!(!api.is_net_short(&instrument_id));
        assert!(api.is_flat(&instrument_id));
        assert!(api.is_completely_flat());
        assert!(api.recorded_realized_pnls().is_empty());

        let _statistics = api.statistics();
    }
}

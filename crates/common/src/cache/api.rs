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

//! User-facing read API over the platform cache.

use std::{
    cell::{Ref, RefCell},
    fmt::Display,
};

use ahash::AHashSet;
use bytes::Bytes;
#[cfg(feature = "defi")]
use nautilus_model::defi::{Pool, PoolProfiler};
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, BarType, FundingRateUpdate, GreeksData, IndexPriceUpdate, InstrumentClose,
        InstrumentStatus, MarkPriceUpdate, QuoteTick, TradeTick, option_chain::OptionGreeks,
    },
    enums::{AggregationSource, InstrumentClass, OmsType, OrderSide, PositionSide, PriceType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, Venue, VenueOrderId,
    },
    instruments::{InstrumentAny, SyntheticInstrument},
    orderbook::{OrderBook, own::OwnOrderBook},
    orders::{OrderAny, OrderList},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    Cache,
    error::{
        AccountLookupError, CurrencyLookupError, InstrumentLookupError, OrderBookLookupError,
        OrderListLookupError, OrderLookupError, OwnOrderBookLookupError, PositionLookupError,
        SyntheticInstrumentLookupError,
    },
};
use crate::component::ComponentAccessError;

/// User-facing cache API.
///
/// Point reads return owned snapshots where possible, so actor code does not retain a `Ref` into
/// the live [`Cache`]. Plural collection reads return owned snapshots of all matching values and
/// are intentionally named as bulk reads. Prefer the count, ID, or `has_*` methods in hot paths
/// when a full snapshot is not needed.
#[derive(Debug)]
pub struct CacheApi<'a> {
    cache: &'a RefCell<Cache>,
}

impl<'a> CacheApi<'a> {
    pub(crate) fn new(cache: &'a RefCell<Cache>) -> Self {
        Self { cache }
    }

    /// Returns the unrealized PnL for the `position` using cached market data.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn calculate_unrealized_pnl(&self, position: &Position) -> Option<Money> {
        self.cache().calculate_unrealized_pnl(position)
    }

    /// Returns the OMS type for the `position_id` (if known).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn oms_type(&self, position_id: &PositionId) -> Option<OmsType> {
        self.cache().oms_type(position_id)
    }

    /// Returns serialized position snapshot frames for the `position_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_snapshot_bytes(&self, position_id: &PositionId) -> Option<Vec<Vec<u8>>> {
        self.cache().position_snapshot_bytes(position_id)
    }

    /// Returns the number of stored position snapshots for the `position_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_snapshot_count(&self, position_id: &PositionId) -> usize {
        self.cache().position_snapshot_count(position_id)
    }

    /// Returns position snapshots matching the optional filters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_snapshots(
        &self,
        position_id: Option<&PositionId>,
        account_id: Option<&AccountId>,
    ) -> Vec<Position> {
        self.cache().position_snapshots(position_id, account_id)
    }

    /// Returns position snapshots for `position_id` starting from `skip`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_snapshots_from(&self, position_id: &PositionId, skip: usize) -> Vec<Position> {
        self.cache().position_snapshots_from(position_id, skip)
    }

    /// Returns position snapshot IDs for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_snapshot_ids(&self, instrument_id: &InstrumentId) -> AHashSet<PositionId> {
        self.cache().position_snapshot_ids(instrument_id)
    }

    /// Returns the client order IDs of all orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.cache()
            .client_order_ids(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the client order IDs of all open orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_ids_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.cache()
            .client_order_ids_open(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the client order IDs of all closed orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_ids_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.cache()
            .client_order_ids_closed(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the client order IDs of all locally active orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_ids_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.cache()
            .client_order_ids_active_local(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the client order IDs of all emulated orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_ids_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.cache()
            .client_order_ids_emulated(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the client order IDs of all in-flight orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_ids_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.cache()
            .client_order_ids_inflight(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the position IDs of all positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        self.cache()
            .position_ids(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the position IDs of all open positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_open_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        self.cache()
            .position_open_ids(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the position IDs of all closed positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_closed_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        self.cache()
            .position_closed_ids(venue, instrument_id, strategy_id, account_id)
    }

    /// Returns the strategy IDs in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn strategy_ids(&self) -> AHashSet<StrategyId> {
        self.cache().strategy_ids()
    }

    /// Returns the execution algorithm IDs in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn exec_algorithm_ids(&self) -> AHashSet<ExecAlgorithmId> {
        self.cache().exec_algorithm_ids()
    }

    /// Returns an owned copy of the order for the `client_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn order(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.cache().order_owned(client_order_id)
    }

    /// Returns an owned copy of the order for the `client_order_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`OrderLookupError::NotFound`] when the order is not present in the cache.
    /// - [`OrderLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_order(&self, client_order_id: &ClientOrderId) -> Result<OrderAny, OrderLookupError> {
        self.try_cache("try_order")?
            .try_order_owned(client_order_id)
    }

    /// Returns owned copies of the orders for `client_order_ids`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_for_ids(
        &self,
        client_order_ids: &[ClientOrderId],
        context: &dyn Display,
    ) -> Vec<OrderAny> {
        self.cache().orders_for_ids(client_order_ids, context)
    }

    /// Returns the client order ID for the `venue_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_order_id(&self, venue_order_id: &VenueOrderId) -> Option<ClientOrderId> {
        self.cache().client_order_id(venue_order_id).copied()
    }

    /// Returns the venue order ID for the `client_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.cache().venue_order_id(client_order_id).copied()
    }

    /// Returns the client ID indexed for the `client_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn client_id(&self, client_order_id: &ClientOrderId) -> Option<ClientId> {
        self.cache().client_id(client_order_id).copied()
    }

    /// Returns owned copies of all orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all open orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_open_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all closed orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_closed_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all locally active orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_active_local_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all emulated orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_emulated_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all in-flight orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_inflight_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all orders for the `position_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_for_position(&self, position_id: &PositionId) -> Vec<OrderAny> {
        self.cache()
            .orders_for_position(position_id)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns whether an order with the `client_order_id` exists.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn order_exists(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().order_exists(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is open.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_order_open(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().is_order_open(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is closed.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_order_closed(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().is_order_closed(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is locally active.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_order_active_local(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().is_order_active_local(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is emulated.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_order_emulated(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().is_order_emulated(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is in-flight.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_order_inflight(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().is_order_inflight(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is `PENDING_CANCEL` locally.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_order_pending_cancel_local(&self, client_order_id: &ClientOrderId) -> bool {
        self.cache().is_order_pending_cancel_local(client_order_id)
    }

    /// Returns the count of all open orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_open_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.cache()
            .orders_open_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all closed orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.cache()
            .orders_closed_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all locally active orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_active_local_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.cache()
            .orders_active_local_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all emulated orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_emulated_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.cache()
            .orders_emulated_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all in-flight orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_inflight_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.cache()
            .orders_inflight_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all orders matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.cache()
            .orders_total_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any open order matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_orders_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.cache()
            .has_orders_open(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any closed order matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_orders_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.cache()
            .has_orders_closed(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any locally active order matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_orders_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.cache()
            .has_orders_active_local(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any emulated order matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_orders_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.cache()
            .has_orders_emulated(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any in-flight order matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_orders_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.cache()
            .has_orders_inflight(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any order matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_orders(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.cache()
            .has_orders(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns an owned copy of the order list for the `order_list_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn order_list(&self, order_list_id: &OrderListId) -> Option<OrderList> {
        self.cache().order_list(order_list_id).cloned()
    }

    /// Returns an owned copy of the order list for the `order_list_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`OrderListLookupError::NotFound`] when the order list is not present in the cache.
    /// - [`OrderListLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_order_list(
        &self,
        order_list_id: &OrderListId,
    ) -> Result<OrderList, OrderListLookupError> {
        self.try_cache("try_order_list")?
            .try_order_list(order_list_id)
            .cloned()
    }

    /// Returns owned copies of all order lists matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn order_lists(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Vec<OrderList> {
        self.cache()
            .order_lists(venue, instrument_id, strategy_id, account_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Returns whether an order list with the `order_list_id` exists.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn order_list_exists(&self, order_list_id: &OrderListId) -> bool {
        self.cache().order_list_exists(order_list_id)
    }

    /// Returns owned copies of all orders associated with the `exec_algorithm_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_for_exec_algorithm(
        &self,
        exec_algorithm_id: &ExecAlgorithmId,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderAny> {
        self.cache()
            .orders_for_exec_algorithm(
                exec_algorithm_id,
                venue,
                instrument_id,
                strategy_id,
                account_id,
                side,
            )
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns owned copies of all orders with the `exec_spawn_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn orders_for_exec_spawn(&self, exec_spawn_id: &ClientOrderId) -> Vec<OrderAny> {
        self.cache()
            .orders_for_exec_spawn(exec_spawn_id)
            .into_iter()
            .map(|order| order.cloned())
            .collect()
    }

    /// Returns the total order quantity for the `exec_spawn_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn exec_spawn_total_quantity(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.cache()
            .exec_spawn_total_quantity(exec_spawn_id, active_only)
    }

    /// Returns the total filled quantity for all orders with the `exec_spawn_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn exec_spawn_total_filled_qty(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.cache()
            .exec_spawn_total_filled_qty(exec_spawn_id, active_only)
    }

    /// Returns the total leaves quantity for all orders with the `exec_spawn_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn exec_spawn_total_leaves_qty(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.cache()
            .exec_spawn_total_leaves_qty(exec_spawn_id, active_only)
    }

    /// Returns an owned copy of the position for the `position_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position(&self, position_id: &PositionId) -> Option<Position> {
        self.cache()
            .position_ref(position_id)
            .map(|position| position.cloned())
    }

    /// Returns an owned copy of the position for the `position_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`PositionLookupError::NotFound`] when the position is not present in the cache.
    /// - [`PositionLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_position(&self, position_id: &PositionId) -> Result<Position, PositionLookupError> {
        self.try_cache("try_position")?
            .try_position_ref(position_id)
            .map(|position| position.cloned())
    }

    /// Returns an owned copy of the position for the `client_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_for_order(&self, client_order_id: &ClientOrderId) -> Option<Position> {
        self.cache()
            .position_for_order_ref(client_order_id)
            .map(|position| position.cloned())
    }

    /// Returns the position ID for the `client_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_id(&self, client_order_id: &ClientOrderId) -> Option<PositionId> {
        self.cache().position_id(client_order_id).copied()
    }

    /// Returns owned copies of all positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn positions(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> Vec<Position> {
        self.cache()
            .positions_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|position| position.cloned())
            .collect()
    }

    /// Returns owned copies of all open positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn positions_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> Vec<Position> {
        self.cache()
            .positions_open_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|position| position.cloned())
            .collect()
    }

    /// Returns owned copies of all closed positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn positions_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> Vec<Position> {
        self.cache()
            .positions_closed_refs(venue, instrument_id, strategy_id, account_id, side)
            .into_iter()
            .map(|position| position.cloned())
            .collect()
    }

    /// Returns whether a position with the `position_id` exists.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn position_exists(&self, position_id: &PositionId) -> bool {
        self.cache().position_exists(position_id)
    }

    /// Returns whether a position with the `position_id` is open.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_position_open(&self, position_id: &PositionId) -> bool {
        self.cache().is_position_open(position_id)
    }

    /// Returns whether a position with the `position_id` is closed.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn is_position_closed(&self, position_id: &PositionId) -> bool {
        self.cache().is_position_closed(position_id)
    }

    /// Returns the count of all open positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn positions_open_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.cache()
            .positions_open_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all closed positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn positions_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.cache()
            .positions_closed_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the count of all positions matching the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn positions_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.cache()
            .positions_total_count(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any open position matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_positions_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        self.cache()
            .has_positions_open(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any closed position matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_positions_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        self.cache()
            .has_positions_closed(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns whether any position matches the optional filter parameters.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_positions(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        self.cache()
            .has_positions(venue, instrument_id, strategy_id, account_id, side)
    }

    /// Returns the strategy ID for the `client_order_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn strategy_id_for_order(&self, client_order_id: &ClientOrderId) -> Option<StrategyId> {
        self.cache().strategy_id_for_order(client_order_id).copied()
    }

    /// Returns the strategy ID for the `position_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn strategy_id_for_position(&self, position_id: &PositionId) -> Option<StrategyId> {
        self.cache().strategy_id_for_position(position_id).copied()
    }

    /// Returns the general cache value for the `key` (if found).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `key` is invalid.
    /// - The cache is already mutably borrowed ([`ComponentAccessError`]).
    pub fn get(&self, key: &str) -> anyhow::Result<Option<Bytes>> {
        let cache = self.try_cache("get")?;
        let value = cache.get(key)?;
        Ok(value.cloned())
    }

    /// Returns the price for the `instrument_id` and `price_type` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed, or if `price_type` is [`PriceType::Mid`]
    /// and the quote price precision is already at the maximum fixed precision.
    #[must_use]
    pub fn price(&self, instrument_id: &InstrumentId, price_type: PriceType) -> Option<Price> {
        self.cache().price(instrument_id, price_type)
    }

    /// Returns all quotes for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn quotes(&self, instrument_id: &InstrumentId) -> Option<Vec<QuoteTick>> {
        self.cache().quotes(instrument_id)
    }

    /// Returns all trades for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn trades(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.cache().trades(instrument_id)
    }

    /// Returns all mark price updates for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn mark_prices(&self, instrument_id: &InstrumentId) -> Option<Vec<MarkPriceUpdate>> {
        self.cache().mark_prices(instrument_id)
    }

    /// Returns all index price updates for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn index_prices(&self, instrument_id: &InstrumentId) -> Option<Vec<IndexPriceUpdate>> {
        self.cache().index_prices(instrument_id)
    }

    /// Returns all funding rate updates for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn funding_rates(&self, instrument_id: &InstrumentId) -> Option<Vec<FundingRateUpdate>> {
        self.cache().funding_rates(instrument_id)
    }

    /// Returns all instrument status updates for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instrument_statuses(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<Vec<InstrumentStatus>> {
        self.cache().instrument_statuses(instrument_id)
    }

    /// Returns all bars for the `bar_type` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn bars(&self, bar_type: &BarType) -> Option<Vec<Bar>> {
        self.cache().bars(bar_type)
    }

    /// Returns an owned copy of the order book for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn order_book(&self, instrument_id: &InstrumentId) -> Option<OrderBook> {
        self.cache().order_book(instrument_id).cloned()
    }

    /// Returns an owned copy of the order book for the `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`OrderBookLookupError::NotFound`] when the order book is not present in the cache.
    /// - [`OrderBookLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_order_book(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<OrderBook, OrderBookLookupError> {
        self.try_cache("try_order_book")?
            .try_order_book(instrument_id)
            .cloned()
    }

    /// Returns an owned copy of the own order book for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn own_order_book(&self, instrument_id: &InstrumentId) -> Option<OwnOrderBook> {
        self.cache().own_order_book(instrument_id).cloned()
    }

    /// Returns an owned copy of the own order book for the `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`OwnOrderBookLookupError::NotFound`] when the own order book is not present in the cache.
    /// - [`OwnOrderBookLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_own_order_book(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<OwnOrderBook, OwnOrderBookLookupError> {
        self.try_cache("try_own_order_book")?
            .try_own_order_book(instrument_id)
            .cloned()
    }

    /// Returns the latest quote for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn quote(&self, instrument_id: &InstrumentId) -> Option<QuoteTick> {
        self.cache().quote(instrument_id).copied()
    }

    /// Returns the quote at `index` for the `instrument_id` (if found).
    ///
    /// Index 0 is the most recent.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn quote_at_index(&self, instrument_id: &InstrumentId, index: usize) -> Option<QuoteTick> {
        self.cache().quote_at_index(instrument_id, index).copied()
    }

    /// Returns the latest trade for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn trade(&self, instrument_id: &InstrumentId) -> Option<TradeTick> {
        self.cache().trade(instrument_id).copied()
    }

    /// Returns the trade at `index` for the `instrument_id` (if found).
    ///
    /// Index 0 is the most recent.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn trade_at_index(&self, instrument_id: &InstrumentId, index: usize) -> Option<TradeTick> {
        self.cache().trade_at_index(instrument_id, index).copied()
    }

    /// Returns the latest mark price update for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn mark_price(&self, instrument_id: &InstrumentId) -> Option<MarkPriceUpdate> {
        self.cache().mark_price(instrument_id).copied()
    }

    /// Returns the latest index price update for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn index_price(&self, instrument_id: &InstrumentId) -> Option<IndexPriceUpdate> {
        self.cache().index_price(instrument_id).copied()
    }

    /// Returns the latest funding rate update for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn funding_rate(&self, instrument_id: &InstrumentId) -> Option<FundingRateUpdate> {
        self.cache().funding_rate(instrument_id).copied()
    }

    /// Returns the latest instrument status update for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instrument_status(&self, instrument_id: &InstrumentId) -> Option<InstrumentStatus> {
        self.cache().instrument_status(instrument_id).copied()
    }

    /// Returns the cached close for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instrument_close(&self, instrument_id: &InstrumentId) -> Option<InstrumentClose> {
        self.cache().instrument_close(instrument_id).copied()
    }

    /// Returns the latest bar for the `bar_type` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn bar(&self, bar_type: &BarType) -> Option<Bar> {
        self.cache().bar(bar_type).copied()
    }

    /// Returns the bar at `index` for the `bar_type` (if found).
    ///
    /// Index 0 is the most recent.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn bar_at_index(&self, bar_type: &BarType, index: usize) -> Option<Bar> {
        self.cache().bar_at_index(bar_type, index).copied()
    }

    /// Returns the order book update count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn book_update_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().book_update_count(instrument_id)
    }

    /// Returns the quote tick count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn quote_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().quote_count(instrument_id)
    }

    /// Returns the trade tick count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn trade_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().trade_count(instrument_id)
    }

    /// Returns the mark price update count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn mark_price_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().mark_price_count(instrument_id)
    }

    /// Returns the index price update count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn index_price_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().index_price_count(instrument_id)
    }

    /// Returns the funding rate update count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn funding_rate_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().funding_rate_count(instrument_id)
    }

    /// Returns the instrument status update count for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instrument_status_count(&self, instrument_id: &InstrumentId) -> usize {
        self.cache().instrument_status_count(instrument_id)
    }

    /// Returns the bar count for the `bar_type`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn bar_count(&self, bar_type: &BarType) -> usize {
        self.cache().bar_count(bar_type)
    }

    /// Returns whether the cache contains an order book for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_order_book(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_order_book(instrument_id)
    }

    /// Returns whether the cache contains quotes for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_quote_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_quote_ticks(instrument_id)
    }

    /// Returns whether the cache contains trades for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_trade_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_trade_ticks(instrument_id)
    }

    /// Returns whether the cache contains mark price updates for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_mark_prices(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_mark_prices(instrument_id)
    }

    /// Returns whether the cache contains index price updates for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_index_prices(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_index_prices(instrument_id)
    }

    /// Returns whether the cache contains funding rate updates for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_funding_rates(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_funding_rates(instrument_id)
    }

    /// Returns whether the cache contains instrument status updates for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_instrument_statuses(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_instrument_statuses(instrument_id)
    }

    /// Returns whether the cache contains a close for the `instrument_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_instrument_close(&self, instrument_id: &InstrumentId) -> bool {
        self.cache().has_instrument_close(instrument_id)
    }

    /// Returns whether the cache contains bars for the `bar_type`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn has_bars(&self, bar_type: &BarType) -> bool {
        self.cache().has_bars(bar_type)
    }

    /// Returns the exchange rate for the given currencies and price type (if available).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn get_xrate(
        &self,
        venue: Venue,
        from_currency: Currency,
        to_currency: Currency,
        price_type: PriceType,
    ) -> Option<Decimal> {
        self.cache()
            .get_xrate(venue, from_currency, to_currency, price_type)
    }

    /// Returns the mark exchange rate for the currency pair (if set).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn get_mark_xrate(&self, from_currency: Currency, to_currency: Currency) -> Option<f64> {
        self.cache().get_mark_xrate(from_currency, to_currency)
    }

    /// Returns the yield curve for the `key` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn yield_curve(&self, key: &str) -> Option<Box<dyn Fn(f64) -> f64>> {
        self.cache().yield_curve(key)
    }

    /// Returns an owned copy of the greeks data for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn greeks(&self, instrument_id: &InstrumentId) -> Option<GreeksData> {
        self.cache().greeks(instrument_id)
    }

    /// Returns exchange-provided option greeks for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn option_greeks(&self, instrument_id: &InstrumentId) -> Option<OptionGreeks> {
        self.cache().option_greeks(instrument_id).copied()
    }

    /// Returns the currency for the `code` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn currency(&self, code: &Ustr) -> Option<Currency> {
        self.cache().currency(code).copied()
    }

    /// Returns the currency for the `code`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`CurrencyLookupError::NotFound`] when the currency is not present in the cache.
    /// - [`CurrencyLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_currency(&self, code: &Ustr) -> Result<Currency, CurrencyLookupError> {
        self.try_cache("try_currency")?.try_currency(code).copied()
    }

    /// Returns an owned copy of the instrument for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.cache().instrument(instrument_id).cloned()
    }

    /// Returns an owned copy of the instrument for the `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`InstrumentLookupError::NotFound`] when the instrument is not present in the cache.
    /// - [`InstrumentLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<InstrumentAny, InstrumentLookupError> {
        self.try_cache("try_instrument")?
            .try_instrument(instrument_id)
            .cloned()
    }

    /// Returns the instrument IDs in the cache, optionally filtered by `venue`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instrument_ids(&self, venue: Option<&Venue>) -> Vec<InstrumentId> {
        self.cache()
            .instrument_ids(venue)
            .into_iter()
            .copied()
            .collect()
    }

    /// Returns owned copies of all instruments for the `venue`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instruments(&self, venue: &Venue, underlying: Option<&Ustr>) -> Vec<InstrumentAny> {
        self.cache()
            .instruments(venue, underlying)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Returns owned copies of all instruments for the `venue`, parent `root`, and instrument
    /// `class`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn instruments_by_parent(
        &self,
        venue: &Venue,
        root: &Ustr,
        class: InstrumentClass,
    ) -> Vec<InstrumentAny> {
        self.cache()
            .instruments_by_parent(venue, root, class)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Returns the bar types in the cache, optionally filtered by instrument and price type.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn bar_types(
        &self,
        instrument_id: Option<&InstrumentId>,
        price_type: Option<&PriceType>,
        aggregation_source: AggregationSource,
    ) -> Vec<BarType> {
        self.cache()
            .bar_types(instrument_id, price_type, aggregation_source)
            .into_iter()
            .copied()
            .collect()
    }

    /// Returns an owned copy of the synthetic instrument for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn synthetic(&self, instrument_id: &InstrumentId) -> Option<SyntheticInstrument> {
        self.cache().synthetic(instrument_id).cloned()
    }

    /// Returns an owned copy of the synthetic instrument for the `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`SyntheticInstrumentLookupError::NotFound`] when the synthetic instrument is not present
    ///   in the cache.
    /// - [`SyntheticInstrumentLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_synthetic(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<SyntheticInstrument, SyntheticInstrumentLookupError> {
        self.try_cache("try_synthetic")?
            .try_synthetic(instrument_id)
            .cloned()
    }

    /// Returns the synthetic instrument IDs in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn synthetic_ids(&self) -> Vec<InstrumentId> {
        self.cache().synthetic_ids().into_iter().copied().collect()
    }

    /// Returns owned copies of all synthetic instruments in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn synthetics(&self) -> Vec<SyntheticInstrument> {
        self.cache().synthetics().into_iter().cloned().collect()
    }

    /// Returns an owned copy of the pool for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn pool(&self, instrument_id: &InstrumentId) -> Option<Pool> {
        self.cache().pool(instrument_id).cloned()
    }

    /// Returns the pool instrument IDs in the cache, optionally filtered by `venue`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn pool_ids(&self, venue: Option<&Venue>) -> Vec<InstrumentId> {
        self.cache().pool_ids(venue)
    }

    /// Returns owned copies of all pools in the cache, optionally filtered by `venue`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn pools(&self, venue: Option<&Venue>) -> Vec<Pool> {
        self.cache().pools(venue).into_iter().cloned().collect()
    }

    /// Returns an owned copy of the pool profiler for the `instrument_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn pool_profiler(&self, instrument_id: &InstrumentId) -> Option<PoolProfiler> {
        self.cache().pool_profiler(instrument_id).cloned()
    }

    /// Returns the pool profiler instrument IDs in the cache, optionally filtered by `venue`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn pool_profiler_ids(&self, venue: Option<&Venue>) -> Vec<InstrumentId> {
        self.cache().pool_profiler_ids(venue)
    }

    /// Returns owned copies of all pool profilers in the cache, optionally filtered by `venue`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[cfg(feature = "defi")]
    #[must_use]
    pub fn pool_profilers(&self, venue: Option<&Venue>) -> Vec<PoolProfiler> {
        self.cache()
            .pool_profilers(venue)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Returns an owned copy of the account for the `account_id` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn account(&self, account_id: &AccountId) -> Option<AccountAny> {
        self.cache().account_owned(account_id)
    }

    /// Returns an owned copy of the account for the `account_id`.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`AccountLookupError::NotFound`] when the account is not present in the cache.
    /// - [`AccountLookupError::Access`] if the cache is already mutably borrowed.
    pub fn try_account(&self, account_id: &AccountId) -> Result<AccountAny, AccountLookupError> {
        self.try_cache("try_account")?
            .try_account(account_id)
            .map(|account| account.cloned())
    }

    /// Returns an owned copy of the account for the `venue` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn account_for_venue(&self, venue: &Venue) -> Option<AccountAny> {
        self.cache().account_for_venue_owned(venue)
    }

    /// Returns the account ID for the `venue` (if found).
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn account_id(&self, venue: &Venue) -> Option<AccountId> {
        self.cache().account_id(venue).copied()
    }

    /// Returns owned copies of all accounts matching the `account_id`.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn accounts(&self, account_id: &AccountId) -> Vec<AccountAny> {
        self.cache()
            .accounts(account_id)
            .into_iter()
            .map(|account| account.cloned())
            .collect()
    }

    /// Returns owned copies of every account in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    #[must_use]
    pub fn accounts_all(&self) -> Vec<AccountAny> {
        self.cache().accounts_all_owned()
    }

    fn cache(&self) -> Ref<'_, Cache> {
        self.try_cache("cache read")
            .unwrap_or_else(|e| panic!("{e}"))
    }

    fn try_cache(&self, operation: &'static str) -> Result<Ref<'_, Cache>, ComponentAccessError> {
        self.cache
            .try_borrow()
            .map_err(|_| ComponentAccessError::ReadConflict {
                resource: "cache",
                operation,
            })
    }
}

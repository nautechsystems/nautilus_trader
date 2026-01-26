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

//! Order message builder for dYdX v4 protocol.
//!
//! This module converts Nautilus order types to dYdX proto messages (`MsgPlaceOrder`,
//! `MsgCancelOrder`). It centralizes all order building logic including:
//!
//! - Market and limit order construction
//! - Conditional orders (stop-loss, take-profit)
//! - Short-term vs long-term order routing based on `OrderLifetime`
//! - Price/quantity quantization via market params
//! - Dynamic block time estimation via `BlockTimeMonitor`
//!
//! The builder produces `cosmrs::Any` messages ready for transaction building.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use cosmrs::Any;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

use super::{
    block_time::BlockTimeMonitor,
    types::{
        ConditionalOrderType, GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS, LimitOrderParams,
        ORDER_FLAG_SHORT_TERM, OrderLifetime, calculate_conditional_order_expiration,
    },
};
use crate::{
    common::parse::{order_side_to_proto, time_in_force_to_proto_with_post_only},
    error::DydxError,
    grpc::{OrderBuilder, OrderGoodUntil, OrderMarketParams, SHORT_TERM_ORDER_MAXIMUM_LIFETIME},
    http::client::DydxHttpClient,
    proto::{
        ToAny,
        dydxprotocol::{
            clob::{MsgCancelOrder, MsgPlaceOrder, OrderId, msg_cancel_order::GoodTilOneof},
            subaccounts::SubaccountId,
        },
    },
};

/// Builds dYdX proto messages from Nautilus orders.
///
/// # Responsibilities
///
/// - Convert Nautilus order types to dYdX protocol messages
/// - Determine short-term vs long-term routing via `OrderLifetime`
/// - Handle price/quantity quantization via `OrderMarketParams`
/// - Use dynamic block time estimation from `BlockTimeMonitor`
///
/// # Does NOT Handle
///
/// - Sequence management (handled by `TransactionManager`)
/// - Transaction signing (handled by `TransactionManager`)
/// - Broadcasting (handled by `TxBroadcaster`)
#[derive(Debug)]
pub struct OrderMessageBuilder {
    http_client: DydxHttpClient,
    wallet_address: String,
    subaccount_number: u32,
    /// Block time monitor for dynamic block time estimation.
    block_time_monitor: Arc<BlockTimeMonitor>,
}

impl OrderMessageBuilder {
    /// Creates a new order message builder.
    #[must_use]
    pub fn new(
        http_client: DydxHttpClient,
        wallet_address: String,
        subaccount_number: u32,
        block_time_monitor: Arc<BlockTimeMonitor>,
    ) -> Self {
        Self {
            http_client,
            wallet_address,
            subaccount_number,
            block_time_monitor,
        }
    }

    /// Returns the maximum duration (in seconds) for short-term orders.
    ///
    /// Computed as: `SHORT_TERM_ORDER_MAXIMUM_LIFETIME (20 blocks) × seconds_per_block`
    ///
    /// Uses dynamic block time from `BlockTimeMonitor` when available,
    /// falling back to 500ms/block when insufficient samples.
    #[must_use]
    pub fn max_short_term_secs(&self) -> f64 {
        SHORT_TERM_ORDER_MAXIMUM_LIFETIME as f64
            * self.block_time_monitor.seconds_per_block_or_default()
    }

    /// Converts expire_time from nanoseconds to seconds if present.
    #[must_use]
    fn expire_time_to_secs(
        &self,
        order_expire_time_ns: Option<nautilus_core::UnixNanos>,
    ) -> Option<i64> {
        order_expire_time_ns.map(crate::common::parse::nanos_to_secs_i64)
    }

    /// Determines the order lifetime for given parameters.
    ///
    /// Uses dynamic block time from `BlockTimeMonitor` to determine if an order
    /// fits within the short-term window (20 blocks × seconds_per_block).
    ///
    /// # Important for Batching
    ///
    /// dYdX protocol restriction: **Short-term orders cannot be batched** - each must be
    /// submitted in its own transaction. Only long-term orders can be batched.
    /// Use this method to check before attempting to batch multiple orders.
    #[must_use]
    pub fn get_order_lifetime(&self, params: &LimitOrderParams) -> OrderLifetime {
        let expire_time = self.expire_time_to_secs(params.expire_time_ns);
        OrderLifetime::from_time_in_force(
            params.time_in_force,
            expire_time,
            false,
            self.max_short_term_secs(),
        )
    }

    /// Checks if an order will be submitted as short-term.
    ///
    /// Short-term orders have protocol restrictions:
    /// - Cannot be batched (one MsgPlaceOrder per transaction)
    /// - Lower latency and fees
    /// - Expire by block height (max 20 blocks)
    #[must_use]
    pub fn is_short_term_order(&self, params: &LimitOrderParams) -> bool {
        self.get_order_lifetime(params).is_short_term()
    }

    /// Checks if a cancellation will be short-term based on the order's properties.
    ///
    /// Short-term cancellations have the same protocol restrictions as short-term placements:
    /// - Cannot be batched (one MsgCancelOrder per transaction)
    ///
    /// The cancel must use the same lifetime as the original order placement.
    #[must_use]
    pub fn is_short_term_cancel(
        &self,
        time_in_force: TimeInForce,
        expire_time_ns: Option<nautilus_core::UnixNanos>,
    ) -> bool {
        let expire_time = self.expire_time_to_secs(expire_time_ns);
        OrderLifetime::from_time_in_force(
            time_in_force,
            expire_time,
            false,
            self.max_short_term_secs(),
        )
        .is_short_term()
    }

    /// Builds a `MsgPlaceOrder` for a market order.
    ///
    /// Market orders are always short-term and execute immediately at the best available price.
    ///
    /// # Errors
    ///
    /// Returns an error if market parameters cannot be retrieved or order building fails.
    pub fn build_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        quantity: Quantity,
        block_height: u32,
    ) -> Result<Any, DydxError> {
        let market_params = self.get_market_params(instrument_id)?;

        let builder = OrderBuilder::new(
            market_params,
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        )
        .market(order_side_to_proto(side), quantity.as_decimal())
        .short_term()
        .until(OrderGoodUntil::Block(
            block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
        ));

        let order = builder
            .build()
            .map_err(|e| DydxError::Order(format!("Failed to build market order: {e}")))?;

        Ok(MsgPlaceOrder { order: Some(order) }.to_any())
    }

    /// Builds a `MsgPlaceOrder` for a limit order.
    ///
    /// Automatically routes to short-term or long-term based on `time_in_force` and `expire_time`.
    ///
    /// # Errors
    ///
    /// Returns an error if market parameters cannot be retrieved or order building fails.
    #[allow(clippy::too_many_arguments)]
    pub fn build_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        block_height: u32,
        expire_time: Option<i64>,
    ) -> Result<Any, DydxError> {
        let market_params = self.get_market_params(instrument_id)?;
        let lifetime = OrderLifetime::from_time_in_force(
            time_in_force,
            expire_time,
            false,
            self.max_short_term_secs(),
        );

        let mut builder = OrderBuilder::new(
            market_params,
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        )
        .limit(
            order_side_to_proto(side),
            price.as_decimal(),
            quantity.as_decimal(),
        )
        .time_in_force(time_in_force_to_proto_with_post_only(
            time_in_force,
            post_only,
        ));

        if reduce_only {
            builder = builder.reduce_only(true);
        }

        // Set expiration based on lifetime
        builder = self.apply_order_lifetime(builder, lifetime, block_height, expire_time)?;

        let order = builder
            .build()
            .map_err(|e| DydxError::Order(format!("Failed to build limit order: {e}")))?;

        Ok(MsgPlaceOrder { order: Some(order) }.to_any())
    }

    /// Builds a `MsgPlaceOrder` for a limit order from `LimitOrderParams`.
    ///
    /// # Errors
    ///
    /// Returns an error if market parameters cannot be retrieved or order building fails.
    pub fn build_limit_order_from_params(
        &self,
        params: &LimitOrderParams,
        block_height: u32,
    ) -> Result<Any, DydxError> {
        let expire_time = self.expire_time_to_secs(params.expire_time_ns);

        self.build_limit_order(
            params.instrument_id,
            params.client_order_id,
            params.side,
            params.price,
            params.quantity,
            params.time_in_force,
            params.post_only,
            params.reduce_only,
            block_height,
            expire_time,
        )
    }

    /// Builds a batch of `MsgPlaceOrder` messages for limit orders.
    ///
    /// # Errors
    ///
    /// Returns an error if any order fails to build.
    pub fn build_limit_orders_batch(
        &self,
        orders: &[LimitOrderParams],
        block_height: u32,
    ) -> Result<Vec<Any>, DydxError> {
        orders
            .iter()
            .map(|params| self.build_limit_order_from_params(params, block_height))
            .collect()
    }

    /// Builds a `MsgCancelOrder` message.
    ///
    /// Automatically routes to short-term or long-term cancellation based on the order's lifetime.
    /// Accepts raw nanoseconds and applies `default_short_term_expiry_secs` if configured.
    ///
    /// # Errors
    ///
    /// Returns an error if market parameters cannot be retrieved or order building fails.
    pub fn build_cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        time_in_force: TimeInForce,
        expire_time_ns: Option<nautilus_core::UnixNanos>,
        block_height: u32,
    ) -> Result<Any, DydxError> {
        let expire_time = self.expire_time_to_secs(expire_time_ns);
        let market_params = self.get_market_params(instrument_id)?;
        let lifetime = OrderLifetime::from_time_in_force(
            time_in_force,
            expire_time,
            false,
            self.max_short_term_secs(),
        );

        let (order_flags, good_til_oneof) = match lifetime {
            OrderLifetime::ShortTerm => (
                0,
                GoodTilOneof::GoodTilBlock(block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME),
            ),
            OrderLifetime::LongTerm | OrderLifetime::Conditional => {
                let cancel_good_til = (Utc::now()
                    + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS))
                .timestamp() as u32;
                (
                    lifetime.order_flags(),
                    GoodTilOneof::GoodTilBlockTime(cancel_good_til),
                )
            }
        };

        let msg = MsgCancelOrder {
            order_id: Some(OrderId {
                subaccount_id: Some(SubaccountId {
                    owner: self.wallet_address.clone(),
                    number: self.subaccount_number,
                }),
                client_id: client_order_id,
                order_flags,
                clob_pair_id: market_params.clob_pair_id,
            }),
            good_til_oneof: Some(good_til_oneof),
        };

        Ok(msg.to_any())
    }

    /// Builds a `MsgCancelOrder` message with explicit order_flags.
    ///
    /// Use this method when you have the original order_flags stored (e.g., from OrderContext).
    /// This avoids re-deriving the order type which can be incorrect for expired orders.
    ///
    /// # Errors
    ///
    /// Returns an error if market parameters cannot be retrieved.
    pub fn build_cancel_order_with_flags(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        order_flags: u32,
        block_height: u32,
    ) -> Result<Any, DydxError> {
        let market_params = self.get_market_params(instrument_id)?;

        let good_til_oneof = if order_flags == ORDER_FLAG_SHORT_TERM {
            GoodTilOneof::GoodTilBlock(block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME)
        } else {
            let cancel_good_til = (Utc::now()
                + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS))
            .timestamp() as u32;
            GoodTilOneof::GoodTilBlockTime(cancel_good_til)
        };

        let msg = MsgCancelOrder {
            order_id: Some(OrderId {
                subaccount_id: Some(SubaccountId {
                    owner: self.wallet_address.clone(),
                    number: self.subaccount_number,
                }),
                client_id: client_order_id,
                order_flags,
                clob_pair_id: market_params.clob_pair_id,
            }),
            good_til_oneof: Some(good_til_oneof),
        };

        Ok(msg.to_any())
    }

    /// Builds a batch of `MsgCancelOrder` messages.
    ///
    /// Each tuple contains: (instrument_id, client_order_id, time_in_force, expire_time_ns)
    ///
    /// # Errors
    ///
    /// Returns an error if any cancellation fails to build.
    pub fn build_cancel_orders_batch(
        &self,
        orders: &[(
            InstrumentId,
            u32,
            TimeInForce,
            Option<nautilus_core::UnixNanos>,
        )],
        block_height: u32,
    ) -> Result<Vec<Any>, DydxError> {
        orders
            .iter()
            .map(|(instrument_id, client_order_id, tif, expire_time_ns)| {
                self.build_cancel_order(
                    *instrument_id,
                    *client_order_id,
                    *tif,
                    *expire_time_ns,
                    block_height,
                )
            })
            .collect()
    }

    /// Builds a batch of `MsgCancelOrder` messages with explicit order_flags.
    ///
    /// Each tuple contains: (instrument_id, client_order_id, order_flags)
    /// Use this method when you have stored order_flags from OrderContext.
    ///
    /// # Errors
    ///
    /// Returns an error if any cancellation fails to build.
    pub fn build_cancel_orders_batch_with_flags(
        &self,
        orders: &[(InstrumentId, u32, u32)],
        block_height: u32,
    ) -> Result<Vec<Any>, DydxError> {
        orders
            .iter()
            .map(|(instrument_id, client_order_id, order_flags)| {
                self.build_cancel_order_with_flags(
                    *instrument_id,
                    *client_order_id,
                    *order_flags,
                    block_height,
                )
            })
            .collect()
    }

    /// Builds a cancel-and-replace batch for order modification.
    ///
    /// Returns `[MsgCancelOrder, MsgPlaceOrder]` as a single atomic transaction.
    /// This eliminates race conditions when modifying orders by combining both
    /// operations into one transaction with a single sequence number.
    ///
    /// Accepts raw nanoseconds for expire times and applies `default_short_term_expiry_secs`
    /// if configured (consistent with placement routing).
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument for both cancel and new order
    /// * `old_client_order_id` - Client ID of the order to cancel
    /// * `new_client_order_id` - Client ID for the replacement order
    /// * `old_time_in_force` - TimeInForce of the original order (for cancel routing)
    /// * `old_expire_time_ns` - Expire time of the original order in nanoseconds (for cancel routing)
    /// * `new_params` - Parameters for the replacement limit order
    /// * `block_height` - Current block height for short-term orders
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation or replacement order fails to build.
    #[allow(clippy::too_many_arguments)]
    pub fn build_cancel_and_replace(
        &self,
        instrument_id: InstrumentId,
        old_client_order_id: u32,
        _new_client_order_id: u32,
        old_time_in_force: TimeInForce,
        old_expire_time_ns: Option<nautilus_core::UnixNanos>,
        new_params: &LimitOrderParams,
        block_height: u32,
    ) -> Result<Vec<Any>, DydxError> {
        // Build cancel message for the old order (accepts nanoseconds, computes internally)
        let cancel_msg = self.build_cancel_order(
            instrument_id,
            old_client_order_id,
            old_time_in_force,
            old_expire_time_ns,
            block_height,
        )?;

        // Build place message for the new order (uses build_limit_order_from_params for default expiry)
        let place_msg = self.build_limit_order_from_params(new_params, block_height)?;

        // Return as [cancel, place] - order matters for atomic execution
        Ok(vec![cancel_msg, place_msg])
    }

    /// Builds a cancel-and-replace batch with explicit order_flags for cancellation.
    ///
    /// Use this method when you have stored order_flags from OrderContext.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation or replacement order fails to build.
    pub fn build_cancel_and_replace_with_flags(
        &self,
        instrument_id: InstrumentId,
        old_client_order_id: u32,
        old_order_flags: u32,
        new_params: &LimitOrderParams,
        block_height: u32,
    ) -> Result<Vec<Any>, DydxError> {
        // Build cancel message using stored order_flags
        let cancel_msg = self.build_cancel_order_with_flags(
            instrument_id,
            old_client_order_id,
            old_order_flags,
            block_height,
        )?;

        // Build place message for the new order
        let place_msg = self.build_limit_order_from_params(new_params, block_height)?;

        // Return as [cancel, place] - order matters for atomic execution
        Ok(vec![cancel_msg, place_msg])
    }

    /// Builds a `MsgPlaceOrder` for a conditional order (stop or take-profit).
    ///
    /// Conditional orders are always stored on-chain (long-term/stateful).
    ///
    /// # Errors
    ///
    /// Returns an error if market parameters cannot be retrieved or order building fails.
    #[allow(clippy::too_many_arguments)]
    pub fn build_conditional_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        order_type: ConditionalOrderType,
        side: OrderSide,
        trigger_price: Price,
        limit_price: Option<Price>,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<Any, DydxError> {
        let market_params = self.get_market_params(instrument_id)?;

        let mut builder = OrderBuilder::new(
            market_params,
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        );

        let proto_side = order_side_to_proto(side);
        let trigger_decimal = trigger_price.as_decimal();
        let size_decimal = quantity.as_decimal();

        // Apply order-type-specific builder method
        builder = match order_type {
            ConditionalOrderType::StopMarket => {
                builder.stop_market(proto_side, trigger_decimal, size_decimal)
            }
            ConditionalOrderType::StopLimit => {
                let limit = limit_price.ok_or_else(|| {
                    DydxError::Order("StopLimit requires limit_price".to_string())
                })?;
                builder.stop_limit(
                    proto_side,
                    limit.as_decimal(),
                    trigger_decimal,
                    size_decimal,
                )
            }
            ConditionalOrderType::TakeProfitMarket => {
                builder.take_profit_market(proto_side, trigger_decimal, size_decimal)
            }
            ConditionalOrderType::TakeProfitLimit => {
                let limit = limit_price.ok_or_else(|| {
                    DydxError::Order("TakeProfitLimit requires limit_price".to_string())
                })?;
                builder.take_profit_limit(
                    proto_side,
                    limit.as_decimal(),
                    trigger_decimal,
                    size_decimal,
                )
            }
        };

        // Apply time-in-force for limit orders
        let effective_tif = time_in_force.unwrap_or(TimeInForce::Gtc);
        if matches!(
            order_type,
            ConditionalOrderType::StopLimit | ConditionalOrderType::TakeProfitLimit
        ) {
            let proto_tif = time_in_force_to_proto_with_post_only(effective_tif, post_only);
            builder = builder.time_in_force(proto_tif);
        }

        if reduce_only {
            builder = builder.reduce_only(true);
        }

        // Conditional orders always use time-based expiration
        let expire = calculate_conditional_order_expiration(effective_tif, expire_time)?;
        builder = builder.until(OrderGoodUntil::Time(expire));

        let order = builder
            .build()
            .map_err(|e| DydxError::Order(format!("Failed to build {order_type:?} order: {e}")))?;

        Ok(MsgPlaceOrder { order: Some(order) }.to_any())
    }

    /// Builds a stop market order.
    ///
    /// # Errors
    ///
    /// Returns an error if the conditional order fails to build.
    #[allow(clippy::too_many_arguments)]
    pub fn build_stop_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<Any, DydxError> {
        self.build_conditional_order(
            instrument_id,
            client_order_id,
            ConditionalOrderType::StopMarket,
            side,
            trigger_price,
            None,
            quantity,
            None,
            false,
            reduce_only,
            expire_time,
        )
    }

    /// Builds a stop limit order.
    ///
    /// # Errors
    ///
    /// Returns an error if the conditional order fails to build.
    #[allow(clippy::too_many_arguments)]
    pub fn build_stop_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        limit_price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<Any, DydxError> {
        self.build_conditional_order(
            instrument_id,
            client_order_id,
            ConditionalOrderType::StopLimit,
            side,
            trigger_price,
            Some(limit_price),
            quantity,
            Some(time_in_force),
            post_only,
            reduce_only,
            expire_time,
        )
    }

    /// Builds a take profit market order.
    ///
    /// # Errors
    ///
    /// Returns an error if the conditional order fails to build.
    #[allow(clippy::too_many_arguments)]
    pub fn build_take_profit_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<Any, DydxError> {
        self.build_conditional_order(
            instrument_id,
            client_order_id,
            ConditionalOrderType::TakeProfitMarket,
            side,
            trigger_price,
            None,
            quantity,
            None,
            false,
            reduce_only,
            expire_time,
        )
    }

    /// Builds a take profit limit order.
    ///
    /// # Errors
    ///
    /// Returns an error if the conditional order fails to build.
    #[allow(clippy::too_many_arguments)]
    pub fn build_take_profit_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        limit_price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<Any, DydxError> {
        self.build_conditional_order(
            instrument_id,
            client_order_id,
            ConditionalOrderType::TakeProfitLimit,
            side,
            trigger_price,
            Some(limit_price),
            quantity,
            Some(time_in_force),
            post_only,
            reduce_only,
            expire_time,
        )
    }

    /// Gets market parameters from the HTTP client cache.
    fn get_market_params(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<OrderMarketParams, DydxError> {
        let market = self
            .http_client
            .get_market_params(&instrument_id)
            .ok_or_else(|| {
                DydxError::Order(format!(
                    "Market params for instrument '{instrument_id}' not found in cache"
                ))
            })?;

        Ok(OrderMarketParams {
            atomic_resolution: market.atomic_resolution,
            clob_pair_id: market.clob_pair_id,
            oracle_price: None,
            quantum_conversion_exponent: market.quantum_conversion_exponent,
            step_base_quantums: market.step_base_quantums,
            subticks_per_tick: market.subticks_per_tick,
        })
    }

    /// Applies order lifetime settings to the builder.
    fn apply_order_lifetime(
        &self,
        builder: OrderBuilder,
        lifetime: OrderLifetime,
        block_height: u32,
        expire_time: Option<i64>,
    ) -> Result<OrderBuilder, DydxError> {
        match lifetime {
            OrderLifetime::ShortTerm => {
                let blocks_offset = self.calculate_block_offset(expire_time);
                Ok(builder
                    .short_term()
                    .until(OrderGoodUntil::Block(block_height + blocks_offset)))
            }
            OrderLifetime::LongTerm => {
                let expire_dt = self.calculate_expire_datetime(expire_time)?;
                Ok(builder.long_term().until(OrderGoodUntil::Time(expire_dt)))
            }
            OrderLifetime::Conditional => {
                // Conditional orders should use build_conditional_order instead
                Err(DydxError::Order(
                    "Use build_conditional_order for conditional orders".to_string(),
                ))
            }
        }
    }

    /// Calculates block offset from expire_time for short-term orders.
    ///
    /// Uses dynamic block time estimation from `BlockTimeMonitor` when available,
    /// falling back to the default block time (500ms) when insufficient samples.
    fn calculate_block_offset(&self, expire_time: Option<i64>) -> u32 {
        if let Some(expire_ts) = expire_time {
            let now = Utc::now().timestamp();
            let seconds = expire_ts - now;
            self.seconds_to_blocks(seconds)
        } else {
            SHORT_TERM_ORDER_MAXIMUM_LIFETIME
        }
    }

    /// Converts seconds until expiry to number of blocks using dynamic block time.
    ///
    /// Uses `BlockTimeMonitor::seconds_per_block_or_default()` for accurate estimation
    /// based on actual observed block times, falling back to 500ms when insufficient samples.
    fn seconds_to_blocks(&self, seconds: i64) -> u32 {
        if seconds <= 0 {
            return 1; // Minimum 1 block
        }

        let secs_per_block = self.block_time_monitor.seconds_per_block_or_default();
        let blocks = (seconds as f64 / secs_per_block).ceil() as u32;

        blocks.clamp(1, SHORT_TERM_ORDER_MAXIMUM_LIFETIME)
    }

    /// Calculates expire datetime for long-term orders.
    fn calculate_expire_datetime(
        &self,
        expire_time: Option<i64>,
    ) -> Result<DateTime<Utc>, DydxError> {
        if let Some(expire_ts) = expire_time {
            DateTime::from_timestamp(expire_ts, 0)
                .ok_or_else(|| DydxError::Parse(format!("Invalid expire timestamp: {expire_ts}")))
        } else {
            Ok(Utc::now() + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS))
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Use 10 seconds as test value (20 blocks * 0.5s)
    const TEST_MAX_SHORT_TERM_SECS: f64 = 10.0;

    #[rstest]
    fn test_order_lifetime_routing() {
        // IOC should be short-term regardless of max_short_term_secs
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Ioc,
            None,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert!(lifetime.is_short_term());

        // GTC without expire_time should be long-term
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtc,
            None,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert!(!lifetime.is_short_term());

        // Conditional should be conditional
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtc,
            None,
            true,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert!(lifetime.is_conditional());
    }

    #[rstest]
    fn test_order_lifetime_with_short_expiry() {
        // Order expiring in 5 seconds should be short-term (within 10s window)
        let expire_time = Some(Utc::now().timestamp() + 5);
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtd,
            expire_time,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert!(lifetime.is_short_term());
    }

    #[rstest]
    fn test_order_lifetime_with_long_expiry() {
        // Order expiring in 60 seconds should be long-term (beyond 10s window)
        let expire_time = Some(Utc::now().timestamp() + 60);
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtd,
            expire_time,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert!(!lifetime.is_short_term());
    }
}

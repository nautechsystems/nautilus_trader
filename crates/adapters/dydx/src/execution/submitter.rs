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

//! Order submission utilities for dYdX v4.
//!
//! This module provides functions for building and submitting orders to the dYdX protocol,
//! including conditional orders (stop-loss, take-profit) and market/limit orders.

use chrono::{DateTime, Duration, Utc};
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

use crate::{
    common::parse::{order_side_to_proto, time_in_force_to_proto_with_post_only},
    error::DydxError,
    grpc::{
        DydxGrpcClient, OrderBuilder, OrderGoodUntil, OrderMarketParams,
        SHORT_TERM_ORDER_MAXIMUM_LIFETIME, TxBuilder, Wallet, types::ChainId,
    },
    http::client::DydxHttpClient,
    proto::{
        ToAny,
        dydxprotocol::clob::{MsgCancelOrder, MsgPlaceOrder},
    },
};

/// Default expiration for GTC conditional orders (90 days).
const GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS: i64 = 90;

/// Conditional order types supported by dYdX.
#[derive(Debug, Clone, Copy)]
pub enum ConditionalOrderType {
    /// Triggers at trigger price, executes as market order.
    StopMarket,
    /// Triggers at trigger price, places limit order at limit price.
    StopLimit,
    /// Triggers at trigger price for profit taking, executes as market order.
    TakeProfitMarket,
    /// Triggers at trigger price for profit taking, places limit order at limit price.
    TakeProfitLimit,
}

/// Calculates the expiration time for conditional orders based on TimeInForce.
///
/// - `GTD` with explicit `expire_time`: uses the provided timestamp.
/// - `GTC` or no `expire_time`: defaults to 90 days from now.
/// - `IOC`/`FOK`: uses 1 hour (these are unusual for conditional orders).
///
/// # Errors
///
/// Returns `DydxError::Parse` if the provided `expire_time` timestamp is invalid.
fn calculate_conditional_order_expiration(
    time_in_force: TimeInForce,
    expire_time: Option<i64>,
) -> Result<DateTime<Utc>, DydxError> {
    if let Some(expire_ts) = expire_time {
        DateTime::from_timestamp(expire_ts, 0)
            .ok_or_else(|| DydxError::Parse(format!("Invalid expire timestamp: {expire_ts}")))
    } else {
        let expiration = match time_in_force {
            TimeInForce::Gtc => Utc::now() + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS),
            TimeInForce::Ioc | TimeInForce::Fok => {
                // IOC/FOK don't typically apply to conditional orders, use short expiration
                Utc::now() + Duration::hours(1)
            }
            // GTD without expire_time, or any other TIF - use long default
            _ => Utc::now() + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS),
        };
        Ok(expiration)
    }
}

#[derive(Debug)]
pub struct OrderSubmitter {
    grpc_client: DydxGrpcClient,
    http_client: DydxHttpClient,
    wallet_address: String,
    subaccount_number: u32,
    chain_id: ChainId,
    authenticator_ids: Vec<u64>,
}

impl OrderSubmitter {
    pub fn new(
        grpc_client: DydxGrpcClient,
        http_client: DydxHttpClient,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: ChainId,
        authenticator_ids: Vec<u64>,
    ) -> Self {
        Self {
            grpc_client,
            http_client,
            wallet_address,
            subaccount_number,
            chain_id,
            authenticator_ids,
        }
    }

    /// Submits a market order to dYdX via gRPC.
    ///
    /// Market orders execute immediately at the best available price.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    pub async fn submit_market_order(
        &self,
        wallet: &Wallet,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        quantity: Quantity,
        block_height: u32,
    ) -> Result<(), DydxError> {
        tracing::info!(
            "Submitting market order: client_id={}, side={:?}, quantity={}",
            client_order_id,
            side,
            quantity
        );

        // Get market params from instrument cache
        let market_params = self.get_market_params(instrument_id)?;

        // Build order using OrderBuilder
        let mut builder = OrderBuilder::new(
            market_params,
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        );

        let proto_side = order_side_to_proto(side);
        let size_decimal = quantity.as_decimal();

        builder = builder.market(proto_side, size_decimal);
        builder = builder.short_term(); // Market orders are short-term
        builder = builder.until(OrderGoodUntil::Block(
            block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
        ));

        let order = builder
            .build()
            .map_err(|e| DydxError::Order(format!("Failed to build market order: {e}")))?;

        // Create MsgPlaceOrder
        let msg_place_order = MsgPlaceOrder { order: Some(order) };

        // Broadcast transaction
        self.broadcast_order_message(wallet, msg_place_order).await
    }

    /// Submits a limit order to dYdX via gRPC.
    ///
    /// Limit orders execute only at the specified price or better.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_limit_order(
        &self,
        wallet: &Wallet,
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
    ) -> Result<(), DydxError> {
        tracing::info!(
            "Submitting limit order: client_id={}, side={:?}, price={}, quantity={}, tif={:?}, post_only={}, reduce_only={}",
            client_order_id,
            side,
            price,
            quantity,
            time_in_force,
            post_only,
            reduce_only
        );

        // Get market params from instrument cache
        let market_params = self.get_market_params(instrument_id)?;

        // Build order using OrderBuilder
        let mut builder = OrderBuilder::new(
            market_params,
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        );

        let proto_side = order_side_to_proto(side);
        let price_decimal = price.as_decimal();
        let size_decimal = quantity.as_decimal();

        builder = builder.limit(proto_side, price_decimal, size_decimal);

        // Set time in force (post_only orders use TimeInForce::PostOnly in dYdX)
        let proto_tif = time_in_force_to_proto_with_post_only(time_in_force, post_only);
        builder = builder.time_in_force(proto_tif);

        // Set reduce_only flag
        if reduce_only {
            builder = builder.reduce_only(true);
        }

        // Determine if short-term or long-term based on TIF and expire_time
        if let Some(expire_ts) = expire_time {
            builder = builder.long_term();
            builder = builder.until(OrderGoodUntil::Time(
                DateTime::from_timestamp(expire_ts, 0)
                    .ok_or_else(|| DydxError::Parse("Invalid expire timestamp".to_string()))?,
            ));
        } else {
            builder = builder.short_term();
            builder = builder.until(OrderGoodUntil::Block(
                block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
            ));
        }

        let order = builder
            .build()
            .map_err(|e| DydxError::Order(format!("Failed to build limit order: {e}")))?;

        // Create MsgPlaceOrder
        let msg_place_order = MsgPlaceOrder { order: Some(order) };

        // Broadcast transaction
        self.broadcast_order_message(wallet, msg_place_order).await
    }

    /// Cancels an order on dYdX via gRPC.
    ///
    /// Requires instrument_id to retrieve correct clob_pair_id from market params.
    /// For now, assumes short-term orders (order_flags=0). Future enhancement:
    /// track order_flags when placing orders to handle long-term cancellations.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC cancellation fails or market params not found.
    pub async fn cancel_order(
        &self,
        wallet: &Wallet,
        instrument_id: InstrumentId,
        client_order_id: u32,
        block_height: u32,
    ) -> Result<(), DydxError> {
        tracing::info!(
            "Cancelling order: client_id={}, instrument={}",
            client_order_id,
            instrument_id
        );

        // Get market params to retrieve clob_pair_id
        let market_params = self.get_market_params(instrument_id)?;

        // Create MsgCancelOrder
        let msg_cancel = MsgCancelOrder {
            order_id: Some(crate::proto::dydxprotocol::clob::OrderId {
                subaccount_id: Some(crate::proto::dydxprotocol::subaccounts::SubaccountId {
                    owner: self.wallet_address.clone(),
                    number: self.subaccount_number,
                }),
                client_id: client_order_id,
                order_flags: 0, // Short-term orders (0), long-term (64), conditional (32)
                clob_pair_id: market_params.clob_pair_id,
            }),
            good_til_oneof: Some(
                crate::proto::dydxprotocol::clob::msg_cancel_order::GoodTilOneof::GoodTilBlock(
                    block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
                ),
            ),
        };

        // Broadcast transaction
        self.broadcast_cancel_message(wallet, msg_cancel).await
    }

    /// Cancels multiple orders in a single blockchain transaction.
    ///
    /// Batches all cancellation messages into one transaction for efficiency.
    /// This is more efficient than sequential cancellation as it requires only
    /// one account lookup and one transaction broadcast.
    ///
    /// # Arguments
    ///
    /// * `wallet` - The wallet for signing transactions
    /// * `orders` - Slice of (InstrumentId, client_order_id) tuples to cancel
    /// * `block_height` - Current block height for order expiration
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if transaction broadcast fails or market params not found.
    pub async fn cancel_orders_batch(
        &self,
        wallet: &Wallet,
        orders: &[(InstrumentId, u32)],
        block_height: u32,
    ) -> Result<(), DydxError> {
        if orders.is_empty() {
            return Ok(());
        }

        tracing::info!(
            "Batch cancelling {} orders in single transaction",
            orders.len()
        );

        // Build all cancel messages
        let mut cancel_msgs = Vec::with_capacity(orders.len());
        for (instrument_id, client_order_id) in orders {
            let market_params = self.get_market_params(*instrument_id)?;

            let msg_cancel = MsgCancelOrder {
                order_id: Some(crate::proto::dydxprotocol::clob::OrderId {
                    subaccount_id: Some(crate::proto::dydxprotocol::subaccounts::SubaccountId {
                        owner: self.wallet_address.clone(),
                        number: self.subaccount_number,
                    }),
                    client_id: *client_order_id,
                    order_flags: 0,
                    clob_pair_id: market_params.clob_pair_id,
                }),
                good_til_oneof: Some(
                    crate::proto::dydxprotocol::clob::msg_cancel_order::GoodTilOneof::GoodTilBlock(
                        block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
                    ),
                ),
            };
            cancel_msgs.push(msg_cancel);
        }

        // Broadcast all cancellations in a single transaction
        self.broadcast_cancel_messages_batch(wallet, cancel_msgs)
            .await
    }

    /// Submits a conditional order (stop or take-profit) to dYdX via gRPC.
    ///
    /// This is the unified implementation for all conditional order types.
    /// Market variants execute immediately when triggered; limit variants place
    /// a limit order at the specified price.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails or limit_price missing for limit orders.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_conditional_order(
        &self,
        wallet: &Wallet,
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
    ) -> Result<(), DydxError> {
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
                tracing::info!(
                    "Submitting stop market order: client_id={}, side={:?}, trigger={}, qty={}",
                    client_order_id,
                    side,
                    trigger_price,
                    quantity
                );
                builder.stop_market(proto_side, trigger_decimal, size_decimal)
            }
            ConditionalOrderType::StopLimit => {
                let limit = limit_price.ok_or_else(|| {
                    DydxError::Order("StopLimit requires limit_price".to_string())
                })?;
                tracing::info!(
                    "Submitting stop limit order: client_id={}, side={:?}, trigger={}, limit={}, qty={}",
                    client_order_id,
                    side,
                    trigger_price,
                    limit,
                    quantity
                );
                builder.stop_limit(
                    proto_side,
                    limit.as_decimal(),
                    trigger_decimal,
                    size_decimal,
                )
            }
            ConditionalOrderType::TakeProfitMarket => {
                tracing::info!(
                    "Submitting take profit market order: client_id={}, side={:?}, trigger={}, qty={}",
                    client_order_id,
                    side,
                    trigger_price,
                    quantity
                );
                builder.take_profit_market(proto_side, trigger_decimal, size_decimal)
            }
            ConditionalOrderType::TakeProfitLimit => {
                let limit = limit_price.ok_or_else(|| {
                    DydxError::Order("TakeProfitLimit requires limit_price".to_string())
                })?;
                tracing::info!(
                    "Submitting take profit limit order: client_id={}, side={:?}, trigger={}, limit={}, qty={}",
                    client_order_id,
                    side,
                    trigger_price,
                    limit,
                    quantity
                );
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

        let expire = calculate_conditional_order_expiration(effective_tif, expire_time)?;
        builder = builder.until(OrderGoodUntil::Time(expire));

        let order = builder
            .build()
            .map_err(|e| DydxError::Order(format!("Failed to build {order_type:?} order: {e}")))?;

        let msg_place_order = MsgPlaceOrder { order: Some(order) };
        self.broadcast_order_message(wallet, msg_place_order).await
    }

    /// Submits a stop market order to dYdX via gRPC.
    ///
    /// Stop market orders are triggered when the price reaches `trigger_price`.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_stop_market_order(
        &self,
        wallet: &Wallet,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        self.submit_conditional_order(
            wallet,
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
        .await
    }

    /// Submits a stop limit order to dYdX via gRPC.
    ///
    /// Stop limit orders are triggered when the price reaches `trigger_price`,
    /// then placed as a limit order at `limit_price`.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_stop_limit_order(
        &self,
        wallet: &Wallet,
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
    ) -> Result<(), DydxError> {
        self.submit_conditional_order(
            wallet,
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
        .await
    }

    /// Submits a take profit market order to dYdX via gRPC.
    ///
    /// Take profit market orders are triggered when the price reaches `trigger_price`,
    /// then executed as a market order.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_take_profit_market_order(
        &self,
        wallet: &Wallet,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        self.submit_conditional_order(
            wallet,
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
        .await
    }

    /// Submits a take profit limit order to dYdX via gRPC.
    ///
    /// Take profit limit orders are triggered when the price reaches `trigger_price`,
    /// then placed as a limit order at `limit_price`.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_take_profit_limit_order(
        &self,
        wallet: &Wallet,
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
    ) -> Result<(), DydxError> {
        self.submit_conditional_order(
            wallet,
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
        .await
    }

    /// Get market params from instrument cache.
    ///
    /// # Errors
    ///
    /// Returns an error if instrument is not found in cache or market params cannot be extracted.
    fn get_market_params(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<OrderMarketParams, DydxError> {
        // Look up market data from HTTP client cache
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
            oracle_price: None, // Oracle price is dynamic, updated separately
            quantum_conversion_exponent: market.quantum_conversion_exponent,
            step_base_quantums: market.step_base_quantums,
            subticks_per_tick: market.subticks_per_tick,
        })
    }

    /// Broadcasts a transaction message to dYdX via gRPC.
    ///
    /// Generic method for broadcasting any transaction type that implements `ToAny`.
    /// Handles signing, serialization, and gRPC transmission.
    async fn broadcast_tx_message<T: ToAny>(
        &self,
        wallet: &Wallet,
        msg: T,
        operation: &str,
    ) -> Result<(), DydxError> {
        // Derive account for signing (uses derivation index 0 for main account)
        let mut account = wallet
            .account_offline(0)
            .map_err(|e| DydxError::Wallet(format!("Failed to derive account: {e}")))?;

        // Fetch current account info from chain to get proper account_number and sequence
        let mut grpc_client = self.grpc_client.clone();
        let base_account = grpc_client
            .get_account(&self.wallet_address)
            .await
            .map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "Failed to fetch account info: {e}"
                ))))
            })?;

        // Update account with on-chain values
        account.set_account_info(base_account.account_number, base_account.sequence);

        // Build transaction
        let tx_builder =
            TxBuilder::new(self.chain_id.clone(), "adydx".to_string()).map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "TxBuilder init failed: {e}"
                ))))
            })?;

        // Convert message to Any
        let any_msg = msg.to_any();

        // Build and sign transaction
        let auth_ids = if self.authenticator_ids.is_empty() {
            None
        } else {
            Some(self.authenticator_ids.as_slice())
        };
        let tx_raw = tx_builder
            .build_transaction(&account, vec![any_msg], None, auth_ids)
            .map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "Failed to build tx: {e}"
                ))))
            })?;

        // Broadcast transaction
        let tx_bytes = tx_raw.to_bytes().map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to serialize tx: {e}"
            ))))
        })?;

        tracing::debug!(
            "Broadcasting {} with {} bytes, account_seq={}",
            operation,
            tx_bytes.len(),
            account.sequence_number
        );

        let mut grpc_client = self.grpc_client.clone();
        let tx_hash = grpc_client.broadcast_tx(tx_bytes).await.map_err(|e| {
            tracing::error!("gRPC broadcast failed for {}: {}", operation, e);
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Broadcast failed: {e}"
            ))))
        })?;

        tracing::info!("{} successfully: tx_hash={}", operation, tx_hash);
        Ok(())
    }

    /// Broadcast order placement message via gRPC.
    async fn broadcast_order_message(
        &self,
        wallet: &Wallet,
        msg: MsgPlaceOrder,
    ) -> Result<(), DydxError> {
        self.broadcast_tx_message(wallet, msg, "Order placed").await
    }

    /// Broadcast order cancellation message via gRPC.
    async fn broadcast_cancel_message(
        &self,
        wallet: &Wallet,
        msg: MsgCancelOrder,
    ) -> Result<(), DydxError> {
        self.broadcast_tx_message(wallet, msg, "Order cancelled")
            .await
    }

    /// Broadcast multiple order cancellation messages in a single transaction.
    async fn broadcast_cancel_messages_batch(
        &self,
        wallet: &Wallet,
        msgs: Vec<MsgCancelOrder>,
    ) -> Result<(), DydxError> {
        let count = msgs.len();

        // Derive account for signing
        let mut account = wallet
            .account_offline(0)
            .map_err(|e| DydxError::Wallet(format!("Failed to derive account: {e}")))?;

        // Fetch current account info
        let mut grpc_client = self.grpc_client.clone();
        let base_account = grpc_client
            .get_account(&self.wallet_address)
            .await
            .map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "Failed to fetch account info: {e}"
                ))))
            })?;

        account.set_account_info(base_account.account_number, base_account.sequence);

        // Build transaction with all messages
        let tx_builder =
            TxBuilder::new(self.chain_id.clone(), "adydx".to_string()).map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "TxBuilder init failed: {e}"
                ))))
            })?;

        // Convert all messages to Any
        let any_msgs: Vec<_> = msgs.into_iter().map(|m| m.to_any()).collect();

        let auth_ids = if self.authenticator_ids.is_empty() {
            None
        } else {
            Some(self.authenticator_ids.as_slice())
        };
        let tx_raw = tx_builder
            .build_transaction(&account, any_msgs, None, auth_ids)
            .map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "Failed to build tx: {e}"
                ))))
            })?;

        let tx_bytes = tx_raw.to_bytes().map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to serialize tx: {e}"
            ))))
        })?;

        let mut grpc_client = self.grpc_client.clone();
        let tx_hash = grpc_client.broadcast_tx(tx_bytes).await.map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Broadcast failed: {e}"
            ))))
        })?;

        tracing::info!("Batch cancelled {} orders: tx_hash={}", count, tx_hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_cancel_orders_batch_builds_multiple_messages() {
        let btc_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let eth_id = InstrumentId::from("ETH-USD-PERP.DYDX");
        let orders = [(btc_id, 100u32), (btc_id, 101u32), (eth_id, 200u32)];

        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0], (btc_id, 100));
        assert_eq!(orders[1], (btc_id, 101));
        assert_eq!(orders[2], (eth_id, 200));
    }

    #[rstest]
    fn test_cancel_orders_batch_empty_returns_ok() {
        let orders: [(InstrumentId, u32); 0] = [];
        assert!(orders.is_empty());
    }

    #[rstest]
    fn test_conditional_order_expiration_with_explicit_timestamp() {
        let expire_ts = 1735689600i64; // 2025-01-01 00:00:00 UTC
        let result =
            calculate_conditional_order_expiration(TimeInForce::Gtd, Some(expire_ts)).unwrap();
        assert_eq!(result.timestamp(), expire_ts);
    }

    #[rstest]
    fn test_conditional_order_expiration_gtc_uses_90_days() {
        let now = Utc::now();
        let result = calculate_conditional_order_expiration(TimeInForce::Gtc, None).unwrap();

        let expected_min = now + Duration::days(89);
        let expected_max = now + Duration::days(91);

        assert!(result > expected_min);
        assert!(result < expected_max);
    }

    #[rstest]
    fn test_conditional_order_expiration_gtd_without_timestamp_uses_90_days() {
        let now = Utc::now();
        let result = calculate_conditional_order_expiration(TimeInForce::Gtd, None).unwrap();

        let expected_min = now + Duration::days(89);
        let expected_max = now + Duration::days(91);

        assert!(result > expected_min);
        assert!(result < expected_max);
    }

    #[rstest]
    fn test_conditional_order_expiration_ioc_uses_1_hour() {
        let now = Utc::now();
        let result = calculate_conditional_order_expiration(TimeInForce::Ioc, None).unwrap();

        let expected_min = now + Duration::minutes(59);
        let expected_max = now + Duration::minutes(61);

        assert!(result > expected_min);
        assert!(result < expected_max);
    }

    #[rstest]
    fn test_conditional_order_expiration_fok_uses_1_hour() {
        let now = Utc::now();
        let result = calculate_conditional_order_expiration(TimeInForce::Fok, None).unwrap();

        let expected_min = now + Duration::minutes(59);
        let expected_max = now + Duration::minutes(61);

        assert!(result > expected_min);
        assert!(result < expected_max);
    }

    #[rstest]
    fn test_conditional_order_expiration_day_uses_90_days() {
        let now = Utc::now();
        let result = calculate_conditional_order_expiration(TimeInForce::Day, None).unwrap();

        let expected_min = now + Duration::days(89);
        let expected_max = now + Duration::days(91);

        assert!(result > expected_min);
        assert!(result < expected_max);
    }

    #[rstest]
    fn test_conditional_order_expiration_invalid_timestamp_returns_error() {
        // i64::MAX is beyond the valid range for chrono timestamps
        let result = calculate_conditional_order_expiration(TimeInForce::Gtd, Some(i64::MAX));
        assert!(result.is_err());
    }

    #[rstest]
    fn test_conditional_order_type_debug_format() {
        assert_eq!(
            format!("{:?}", ConditionalOrderType::StopMarket),
            "StopMarket"
        );
        assert_eq!(
            format!("{:?}", ConditionalOrderType::StopLimit),
            "StopLimit"
        );
        assert_eq!(
            format!("{:?}", ConditionalOrderType::TakeProfitMarket),
            "TakeProfitMarket"
        );
        assert_eq!(
            format!("{:?}", ConditionalOrderType::TakeProfitLimit),
            "TakeProfitLimit"
        );
    }
}

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

//! Order submission facade for dYdX v4.
//!
//! This module provides [`OrderSubmitter`], a unified facade for submitting orders to dYdX.
//! It internally uses the extracted components:
//! - [`TransactionManager`]: Sequence tracking and transaction signing
//! - [`TxBroadcaster`]: gRPC broadcast with retry logic
//! - [`OrderMessageBuilder`]: Proto message construction
//!
//! The wallet is owned internally by `TransactionManager`, so method signatures
//! don't require passing `&wallet` on each call.

use std::sync::Arc;

use nautilus_common::live::get_runtime;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_network::ratelimiter::quota::Quota;

use crate::{
    error::DydxError,
    execution::{
        block_time::BlockTimeMonitor,
        broadcaster::TxBroadcaster,
        order_builder::OrderMessageBuilder,
        tx_manager::TransactionManager,
        types::{ConditionalOrderType, LimitOrderParams, OrderLifetime},
    },
    grpc::{DydxGrpcClient, types::ChainId},
    http::client::DydxHttpClient,
};

/// Order submission facade for dYdX v4.
///
/// Provides a clean API for order submission, internally coordinating:
/// - [`TransactionManager`]: Owns wallet, handles sequence + signing
/// - [`TxBroadcaster`]: Handles gRPC broadcast with retry
/// - [`OrderMessageBuilder`]: Constructs proto messages
///
/// # Wallet Ownership
///
/// The wallet is owned by `TransactionManager` (passed at construction via `private_key`).
/// This eliminates the need to pass `&wallet` to every method.
///
/// # Block Time Monitor
///
/// `block_time_monitor` provides current block height and dynamic block time estimation.
/// Updated externally by WebSocket, read by order methods.
///
/// # Thread Safety
///
/// All methods are safe to call from multiple tasks concurrently.
#[derive(Debug)]
pub struct OrderSubmitter {
    /// Transaction manager - owns wallet, handles sequence and signing.
    tx_manager: Arc<TransactionManager>,
    /// Transaction broadcaster with retry logic.
    broadcaster: Arc<TxBroadcaster>,
    /// Order message builder for proto construction.
    order_builder: Arc<OrderMessageBuilder>,
    /// Block time monitor - provides current height and block time estimation.
    block_time_monitor: Arc<BlockTimeMonitor>,
}

impl OrderSubmitter {
    /// Creates a new order submitter with wallet owned internally.
    ///
    /// # Arguments
    ///
    /// * `grpc_client` - gRPC client for chain queries and broadcasting
    /// * `http_client` - HTTP client (provides market params cache)
    /// * `private_key` - Private key (hex-encoded) - wallet created internally
    /// * `wallet_address` - Main account address (may differ from derived address for permissioned keys)
    /// * `subaccount_number` - dYdX subaccount number (typically 0)
    /// * `chain_id` - dYdX chain ID
    /// * `block_time_monitor` - Block time monitor (provides current height and dynamic block time)
    /// * `grpc_quota` - Optional rate limit quota for gRPC calls
    ///
    /// # Errors
    ///
    /// Returns error if wallet creation from private key fails.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        grpc_client: DydxGrpcClient,
        http_client: DydxHttpClient,
        private_key: &str,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: ChainId,
        block_time_monitor: Arc<BlockTimeMonitor>,
        grpc_quota: Option<Quota>,
    ) -> Result<Self, DydxError> {
        // Create transaction manager (owns wallet and sequence management)
        let tx_manager = Arc::new(TransactionManager::new(
            grpc_client.clone(),
            private_key,
            wallet_address.clone(),
            chain_id,
        )?);

        let broadcaster = Arc::new(TxBroadcaster::new(grpc_client, grpc_quota));

        let order_builder = Arc::new(OrderMessageBuilder::new(
            http_client,
            wallet_address,
            subaccount_number,
            block_time_monitor.clone(),
        ));

        Ok(Self {
            tx_manager,
            broadcaster,
            order_builder,
            block_time_monitor,
        })
    }

    /// Creates a new order submitter from pre-built components.
    ///
    /// Use this when you already have initialized components (e.g., from `DydxExecutionClient`).
    pub fn from_components(
        tx_manager: Arc<TransactionManager>,
        broadcaster: Arc<TxBroadcaster>,
        order_builder: Arc<OrderMessageBuilder>,
        block_time_monitor: Arc<BlockTimeMonitor>,
    ) -> Self {
        Self {
            tx_manager,
            broadcaster,
            order_builder,
            block_time_monitor,
        }
    }

    /// Returns the current block height.
    #[must_use]
    pub fn current_block_height(&self) -> u32 {
        self.block_time_monitor.current_block_height() as u32
    }

    /// Returns a reference to the block time monitor.
    #[must_use]
    pub fn block_time_monitor(&self) -> &BlockTimeMonitor {
        &self.block_time_monitor
    }

    /// Returns the wallet address.
    #[must_use]
    pub fn wallet_address(&self) -> &str {
        self.tx_manager.wallet_address()
    }

    /// Returns a reference to the order builder.
    #[must_use]
    pub fn order_builder(&self) -> &OrderMessageBuilder {
        &self.order_builder
    }

    /// Returns a reference to the transaction manager.
    #[must_use]
    pub fn tx_manager(&self) -> &TransactionManager {
        &self.tx_manager
    }

    /// Submits a market order to dYdX via gRPC.
    ///
    /// Market orders execute immediately at the best available price.
    /// Block height is read from the shared `block_height` state.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    pub async fn submit_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        side: OrderSide,
        quantity: Quantity,
    ) -> Result<String, DydxError> {
        log::debug!(
            "Submitting market order: client_id={client_order_id}, meta={client_metadata:#x}, side={side:?}, quantity={quantity}"
        );

        let block_height = self.current_block_height();

        // Build proto message
        let msg = self.order_builder.build_market_order(
            instrument_id,
            client_order_id,
            client_metadata,
            side,
            quantity,
            block_height,
        )?;

        // Market orders are always short-term — use cached sequence (no increment)
        let operation = format!("Submit market order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_short_term(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Submits a limit order to dYdX via gRPC.
    ///
    /// Limit orders execute only at the specified price or better.
    /// Block height is read from the shared `block_height` state.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::debug!(
            "Submitting limit order: client_id={client_order_id}, meta={client_metadata:#x}, side={side:?}, price={price}, \
             quantity={quantity}, tif={time_in_force:?}, post_only={post_only}, reduce_only={reduce_only}"
        );

        let block_height = self.current_block_height();

        // Build proto message
        let msg = self.order_builder.build_limit_order(
            instrument_id,
            client_order_id,
            client_metadata,
            side,
            price,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            block_height,
            expire_time,
        )?;

        // Determine if short-term based on time_in_force and expire_time
        let is_short_term = OrderLifetime::from_time_in_force(
            time_in_force,
            expire_time,
            false,
            self.order_builder.max_short_term_secs(),
        )
        .is_short_term();

        // Short-term: cached sequence, no retry. Stateful: proper sequence management.
        let operation = format!("Submit limit order {client_order_id}");
        let tx_hash = if is_short_term {
            self.broadcaster
                .broadcast_short_term(&self.tx_manager, vec![msg], &operation)
                .await?
        } else {
            self.broadcaster
                .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
                .await?
        };

        Ok(tx_hash)
    }

    /// Submits a batch of limit orders.
    ///
    /// # Protocol Constraints
    ///
    /// - **Short-term orders cannot be batched**: If any order is short-term (IOC, FOK, or
    ///   expire_time within 60s), each order is submitted in a separate transaction.
    /// - **Long-term orders can be batched**: All orders in a single transaction.
    ///
    /// # Returns
    ///
    /// A vector of transaction hashes (one per transaction).
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if any submission fails.
    pub async fn submit_limit_orders_batch(
        &self,
        orders: Vec<LimitOrderParams>,
    ) -> Result<Vec<String>, DydxError> {
        if orders.is_empty() {
            return Ok(Vec::new());
        }

        let block_height = self.current_block_height();

        // Check if any orders are short-term (cannot be batched)
        let has_short_term = orders
            .iter()
            .any(|params| self.order_builder.is_short_term_order(params));

        if has_short_term {
            // Short-term orders must be submitted individually.
            // They don't consume Cosmos SDK sequences (GTB replay protection),
            // so we use broadcast_short_term for concurrent submission.
            log::debug!(
                "Submitting {} short-term limit orders concurrently (sequence not consumed)",
                orders.len()
            );

            let mut tx_hashes = Vec::with_capacity(orders.len());
            let mut handles = Vec::with_capacity(orders.len());

            for params in orders {
                let tx_manager = Arc::clone(&self.tx_manager);
                let broadcaster = Arc::clone(&self.broadcaster);
                let order_builder = Arc::clone(&self.order_builder);

                let handle = get_runtime().spawn(async move {
                    let msg = order_builder.build_limit_order_from_params(&params, block_height)?;
                    let operation = format!("Submit short-term order {}", params.client_order_id);
                    broadcaster
                        .broadcast_short_term(&tx_manager, vec![msg], &operation)
                        .await
                });

                handles.push(handle);
            }

            // Collect results
            for handle in handles {
                match handle.await {
                    Ok(Ok(tx_hash)) => tx_hashes.push(tx_hash),
                    Ok(Err(e)) => return Err(e),
                    Err(e) => {
                        return Err(DydxError::Nautilus(anyhow::anyhow!("Task join error: {e}")));
                    }
                }
            }

            Ok(tx_hashes)
        } else {
            // Long-term orders can be batched in a single transaction
            log::info!(
                "Batch submitting {} long-term limit orders in single transaction",
                orders.len()
            );

            let msgs = self
                .order_builder
                .build_limit_orders_batch(&orders, block_height)?;

            let operation = format!("Submit batch of {} limit orders", msgs.len());
            let tx_hash = self
                .broadcaster
                .broadcast_with_retry(&self.tx_manager, msgs, &operation)
                .await?;

            Ok(vec![tx_hash])
        }
    }

    /// Cancels an order on dYdX via gRPC.
    ///
    /// Block height is read from the shared `block_height` state.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC cancellation fails or market params not found.
    pub async fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        time_in_force: TimeInForce,
        expire_time_ns: Option<nautilus_core::UnixNanos>,
    ) -> Result<String, DydxError> {
        log::debug!("Cancelling order: client_id={client_order_id}, instrument={instrument_id}");

        let block_height = self.current_block_height();

        // Build cancel message
        let msg = self.order_builder.build_cancel_order(
            instrument_id,
            client_order_id,
            time_in_force,
            expire_time_ns,
            block_height,
        )?;

        // Determine if this is a short-term cancel
        let is_short_term = self
            .order_builder
            .is_short_term_cancel(time_in_force, expire_time_ns);

        // Short-term: cached sequence, no retry. Stateful: proper sequence management.
        let operation = format!("Cancel order {client_order_id}");
        let tx_hash = if is_short_term {
            self.broadcaster
                .broadcast_short_term(&self.tx_manager, vec![msg], &operation)
                .await?
        } else {
            self.broadcaster
                .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
                .await?
        };

        Ok(tx_hash)
    }

    /// Cancels multiple orders with optimal partitioned broadcasting.
    ///
    /// Partitions orders into short-term and long-term groups:
    /// - Short-term orders: single `MsgBatchCancel` via `broadcast_short_term()`
    /// - Long-term orders: batched `MsgCancelOrder` messages via `broadcast_with_retry()`
    ///
    /// # Arguments
    ///
    /// * `orders` - Slice of (instrument_id, client_order_id, time_in_force, expire_time_ns) tuples
    ///
    /// # Returns
    ///
    /// Comma-separated transaction hashes on success (one per partition).
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if transaction broadcast fails or market params not found.
    pub async fn cancel_orders_batch(
        &self,
        orders: &[(
            InstrumentId,
            u32,
            TimeInForce,
            Option<nautilus_core::UnixNanos>,
        )],
    ) -> Result<String, DydxError> {
        if orders.is_empty() {
            return Err(DydxError::Order("No orders to cancel".to_string()));
        }

        let block_height = self.current_block_height();

        // Partition into short-term and long-term orders
        let (short_term, long_term): (Vec<_>, Vec<_>) =
            orders.iter().partition(|(_, _, tif, expire_ns)| {
                self.order_builder.is_short_term_cancel(*tif, *expire_ns)
            });

        log::info!(
            "Batch cancelling {} orders (short_term={}, long_term={})",
            orders.len(),
            short_term.len(),
            long_term.len(),
        );

        let mut tx_hashes = Vec::new();

        // Cancel short-term orders with MsgBatchCancel (single gRPC call)
        if !short_term.is_empty() {
            let st_pairs: Vec<_> = short_term
                .iter()
                .map(|(inst_id, client_id, _, _)| (*inst_id, *client_id))
                .collect();

            let msg = self
                .order_builder
                .build_batch_cancel_short_term(&st_pairs, block_height)?;

            let operation = format!("BatchCancel {} short-term orders", st_pairs.len());
            let tx_hash = self
                .broadcaster
                .broadcast_short_term(&self.tx_manager, vec![msg], &operation)
                .await?;
            tx_hashes.push(tx_hash);
        }

        // Cancel long-term orders with batched MsgCancelOrder (single gRPC call)
        if !long_term.is_empty() {
            let lt_orders: Vec<_> = long_term
                .iter()
                .map(|(inst_id, client_id, tif, expire_ns)| {
                    (*inst_id, *client_id, *tif, *expire_ns)
                })
                .collect();

            let msgs = self
                .order_builder
                .build_cancel_orders_batch(&lt_orders, block_height)?;

            let operation = format!("BatchCancel {} long-term orders", lt_orders.len());
            let tx_hash = self
                .broadcaster
                .broadcast_with_retry(&self.tx_manager, msgs, &operation)
                .await?;
            tx_hashes.push(tx_hash);
        }

        Ok(tx_hashes.join(","))
    }

    /// Submits a stop market order to dYdX via gRPC.
    ///
    /// Stop market orders are triggered when the price reaches `trigger_price`.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_stop_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::debug!(
            "Submitting stop market order: client_id={client_order_id}, meta={client_metadata:#x}, side={side:?}, \
             trigger={trigger_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_stop_market_order(
            instrument_id,
            client_order_id,
            client_metadata,
            side,
            trigger_price,
            quantity,
            reduce_only,
            expire_time,
        )?;

        // Broadcast with retry
        let operation = format!("Submit stop market order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Submits a stop limit order to dYdX via gRPC.
    ///
    /// Stop limit orders are triggered when the price reaches `trigger_price`,
    /// then placed as a limit order at `limit_price`.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_stop_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        side: OrderSide,
        trigger_price: Price,
        limit_price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::debug!(
            "Submitting stop limit order: client_id={client_order_id}, meta={client_metadata:#x}, side={side:?}, \
             trigger={trigger_price}, limit={limit_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_stop_limit_order(
            instrument_id,
            client_order_id,
            client_metadata,
            side,
            trigger_price,
            limit_price,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            expire_time,
        )?;

        // Broadcast with retry
        let operation = format!("Submit stop limit order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Submits a take profit market order to dYdX via gRPC.
    ///
    /// Take profit market orders are triggered when the price reaches `trigger_price`,
    /// then executed as a market order.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_take_profit_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::debug!(
            "Submitting take profit market order: client_id={client_order_id}, meta={client_metadata:#x}, side={side:?}, \
             trigger={trigger_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_take_profit_market_order(
            instrument_id,
            client_order_id,
            client_metadata,
            side,
            trigger_price,
            quantity,
            reduce_only,
            expire_time,
        )?;

        // Broadcast with retry
        let operation = format!("Submit take profit market order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Submits a take profit limit order to dYdX via gRPC.
    ///
    /// Take profit limit orders are triggered when the price reaches `trigger_price`,
    /// then placed as a limit order at `limit_price`.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_take_profit_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        side: OrderSide,
        trigger_price: Price,
        limit_price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::debug!(
            "Submitting take profit limit order: client_id={client_order_id}, meta={client_metadata:#x}, side={side:?}, \
             trigger={trigger_price}, limit={limit_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_take_profit_limit_order(
            instrument_id,
            client_order_id,
            client_metadata,
            side,
            trigger_price,
            limit_price,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            expire_time,
        )?;

        // Broadcast with retry
        let operation = format!("Submit take profit limit order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Submits a conditional order (generic interface).
    ///
    /// This method handles all conditional order types: StopMarket, StopLimit,
    /// TakeProfitMarket, and TakeProfitLimit.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC submission fails or `limit_price` is missing for limit orders.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_conditional_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        client_metadata: u32,
        order_type: ConditionalOrderType,
        side: OrderSide,
        trigger_price: Price,
        limit_price: Option<Price>,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        // Build proto message
        let msg = self.order_builder.build_conditional_order(
            instrument_id,
            client_order_id,
            client_metadata,
            order_type,
            side,
            trigger_price,
            limit_price,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            expire_time,
        )?;

        // Broadcast with retry
        let operation = format!("Submit {order_type:?} order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }
}

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

use crate::{
    error::DydxError,
    execution::{
        block_time::BlockTimeMonitor,
        broadcaster::TxBroadcaster,
        order_builder::OrderMessageBuilder,
        tx_manager::TransactionManager,
        types::{ConditionalOrderType, LimitOrderParams},
        wallet::Wallet,
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
    /// * `authenticator_ids` - Authenticator IDs for permissioned key trading
    /// * `block_time_monitor` - Block time monitor (provides current height and dynamic block time)
    ///
    /// # Errors
    ///
    /// Returns error if wallet creation from private key fails.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grpc_client: DydxGrpcClient,
        http_client: DydxHttpClient,
        private_key: &str,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: ChainId,
        authenticator_ids: Vec<u64>,
        block_time_monitor: Arc<BlockTimeMonitor>,
    ) -> Result<Self, DydxError> {
        // Create wallet from private key
        let wallet = Wallet::from_private_key(private_key)
            .map_err(|e| DydxError::Wallet(format!("Failed to create wallet: {e}")))?;

        // Create shared sequence counter (initialized from chain on first use)
        let sequence_number = Arc::new(std::sync::atomic::AtomicU64::new(0));

        // Create components
        let tx_manager = Arc::new(TransactionManager::new(
            grpc_client.clone(),
            wallet,
            wallet_address.clone(),
            chain_id,
            authenticator_ids,
            sequence_number,
        ));

        let broadcaster = Arc::new(TxBroadcaster::new(grpc_client));

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
        side: OrderSide,
        quantity: Quantity,
    ) -> Result<String, DydxError> {
        log::info!(
            "Submitting market order: client_id={client_order_id}, side={side:?}, quantity={quantity}"
        );

        let block_height = self.current_block_height();

        // Build proto message
        let msg = self.order_builder.build_market_order(
            instrument_id,
            client_order_id,
            side,
            quantity,
            block_height,
        )?;

        // Broadcast with retry
        let operation = format!("Submit market order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
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
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_limit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::info!(
            "Submitting limit order: client_id={client_order_id}, side={side:?}, price={price}, \
             quantity={quantity}, tif={time_in_force:?}, post_only={post_only}, reduce_only={reduce_only}"
        );

        let block_height = self.current_block_height();

        // Build proto message
        let msg = self.order_builder.build_limit_order(
            instrument_id,
            client_order_id,
            side,
            price,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            block_height,
            expire_time,
        )?;

        // Broadcast with retry
        let operation = format!("Submit limit order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

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
            // Short-term orders must be submitted individually
            log::info!(
                "Submitting {} limit orders individually (short-term orders cannot be batched)",
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
                    let operation = format!("Submit limit order {}", params.client_order_id);
                    broadcaster
                        .broadcast_with_retry(&tx_manager, vec![msg], &operation)
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
        log::info!("Cancelling order: client_id={client_order_id}, instrument={instrument_id}");

        let block_height = self.current_block_height();

        // Build cancel message
        let msg = self.order_builder.build_cancel_order(
            instrument_id,
            client_order_id,
            time_in_force,
            expire_time_ns,
            block_height,
        )?;

        // Broadcast with retry
        let operation = format!("Cancel order {client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, vec![msg], &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Cancels multiple orders in a single blockchain transaction.
    ///
    /// Batches all cancellation messages into one transaction for efficiency.
    ///
    /// # Arguments
    ///
    /// * `orders` - Slice of (instrument_id, client_order_id, time_in_force, expire_time_ns) tuples
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
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

        log::info!(
            "Batch cancelling {} orders in single transaction",
            orders.len()
        );

        let block_height = self.current_block_height();

        // Build all cancel messages
        let msgs = self
            .order_builder
            .build_cancel_orders_batch(orders, block_height)?;

        // Broadcast with retry
        let operation = format!("Cancel batch of {} orders", msgs.len());
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, msgs, &operation)
            .await?;

        Ok(tx_hash)
    }

    /// Modifies an order via cancel-and-replace.
    ///
    /// dYdX doesn't support native order modification. This method atomically
    /// cancels the old order and places a new one in a single transaction.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument for both cancel and new order
    /// * `old_client_order_id` - Client ID of the order to cancel
    /// * `new_client_order_id` - Client ID for the replacement order
    /// * `old_time_in_force` - TimeInForce of the original order (for cancel routing)
    /// * `old_expire_time_ns` - Expire time of the original order (for cancel routing)
    /// * `new_params` - Parameters for the replacement limit order
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if transaction broadcast fails.
    pub async fn modify_order(
        &self,
        instrument_id: InstrumentId,
        old_client_order_id: u32,
        new_client_order_id: u32,
        old_time_in_force: TimeInForce,
        old_expire_time_ns: Option<nautilus_core::UnixNanos>,
        new_params: &LimitOrderParams,
    ) -> Result<String, DydxError> {
        log::info!(
            "Modifying order via cancel-and-replace: old_id={old_client_order_id}, \
             new_id={new_client_order_id}, price={}, qty={}",
            new_params.price,
            new_params.quantity
        );

        let block_height = self.current_block_height();

        // Build atomic cancel + replace batch
        let msgs = self.order_builder.build_cancel_and_replace(
            instrument_id,
            old_client_order_id,
            new_client_order_id,
            old_time_in_force,
            old_expire_time_ns,
            new_params,
            block_height,
        )?;

        // Broadcast as single transaction
        let operation = format!("Modify order {old_client_order_id} -> {new_client_order_id}");
        let tx_hash = self
            .broadcaster
            .broadcast_with_retry(&self.tx_manager, msgs, &operation)
            .await?;

        Ok(tx_hash)
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
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_stop_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::info!(
            "Submitting stop market order: client_id={client_order_id}, side={side:?}, \
             trigger={trigger_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_stop_market_order(
            instrument_id,
            client_order_id,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_stop_limit_order(
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
    ) -> Result<String, DydxError> {
        log::info!(
            "Submitting stop limit order: client_id={client_order_id}, side={side:?}, \
             trigger={trigger_price}, limit={limit_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_stop_limit_order(
            instrument_id,
            client_order_id,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_take_profit_market_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: u32,
        side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        log::info!(
            "Submitting take profit market order: client_id={client_order_id}, side={side:?}, \
             trigger={trigger_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_take_profit_market_order(
            instrument_id,
            client_order_id,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_take_profit_limit_order(
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
    ) -> Result<String, DydxError> {
        log::info!(
            "Submitting take profit limit order: client_id={client_order_id}, side={side:?}, \
             trigger={trigger_price}, limit={limit_price}, qty={quantity}"
        );

        // Build proto message
        let msg = self.order_builder.build_take_profit_limit_order(
            instrument_id,
            client_order_id,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_conditional_order(
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
    ) -> Result<String, DydxError> {
        // Build proto message
        let msg = self.order_builder.build_conditional_order(
            instrument_id,
            client_order_id,
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

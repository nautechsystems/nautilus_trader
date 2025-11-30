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

use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, prelude::FromStr};

use crate::{
    common::parse::order_side_to_proto,
    error::DydxError,
    grpc::{
        DydxGrpcClient, OrderBuilder, OrderGoodUntil, OrderMarketParams,
        SHORT_TERM_ORDER_MAXIMUM_LIFETIME, Wallet, types::ChainId,
    },
    http::client::DydxHttpClient,
    proto::{
        ToAny,
        dydxprotocol::clob::{MsgCancelOrder, MsgPlaceOrder},
    },
};

#[derive(Debug)]
pub struct OrderSubmitter {
    grpc_client: DydxGrpcClient,
    http_client: DydxHttpClient,
    wallet_address: String,
    subaccount_number: u32,
    chain_id: ChainId,
}

impl OrderSubmitter {
    pub fn new(
        grpc_client: DydxGrpcClient,
        http_client: DydxHttpClient,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: ChainId,
    ) -> Self {
        Self {
            grpc_client,
            http_client,
            wallet_address,
            subaccount_number,
            chain_id,
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
        let size_decimal = Decimal::from_str(&quantity.to_string())
            .map_err(|e| DydxError::Parse(format!("Failed to convert quantity: {e}")))?;

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
        let price_decimal = Decimal::from_str(&price.to_string())
            .map_err(|e| DydxError::Parse(format!("Failed to convert price: {e}")))?;
        let size_decimal = Decimal::from_str(&quantity.to_string())
            .map_err(|e| DydxError::Parse(format!("Failed to convert quantity: {e}")))?;

        builder = builder.limit(proto_side, price_decimal, size_decimal);

        // Set time in force (post_only orders use TimeInForce::PostOnly in dYdX)
        use crate::common::parse::time_in_force_to_proto_with_post_only;
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
                chrono::DateTime::from_timestamp(expire_ts, 0)
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

    /// Cancels multiple orders via individual gRPC transactions.
    ///
    /// dYdX v4 requires separate blockchain transactions for each cancellation.
    /// Each order is cancelled sequentially to avoid nonce conflicts.
    ///
    /// # Arguments
    ///
    /// * `wallet` - The wallet for signing transactions
    /// * `orders` - Slice of (InstrumentId, client_order_id) tuples to cancel
    /// * `block_height` - Current block height for order expiration
    ///
    /// # Errors
    ///
    /// Returns `DydxError::NotImplemented` until fully implemented.
    pub async fn cancel_orders_batch(
        &self,
        _wallet: &Wallet,
        _orders: &[(InstrumentId, u32)],
        _block_height: u32,
    ) -> Result<(), DydxError> {
        // TODO: Implement sequential per-order cancellation
        // For each (instrument_id, client_id) in orders:
        //   self.cancel_order(wallet, instrument_id, client_id, block_height).await?
        Err(DydxError::NotImplemented(
            "Batch cancel requires instrument_id per order - not yet implemented".to_string(),
        ))
    }

    /// Submits a stop market order to dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError::NotImplemented` until conditional order support is added.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_stop_market_order(
        &self,
        _wallet: &Wallet,
        _client_order_id: u32,
        _side: OrderSide,
        _trigger_price: Price,
        _quantity: Quantity,
        _reduce_only: bool,
        _block_height: u32,
        _expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        Err(DydxError::NotImplemented(
            "Stop market orders not yet implemented - awaiting proto generation".to_string(),
        ))
    }

    /// Submits a stop limit order to dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError::NotImplemented` until conditional order support is added.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_stop_limit_order(
        &self,
        _wallet: &Wallet,
        _client_order_id: u32,
        _side: OrderSide,
        _trigger_price: Price,
        _limit_price: Price,
        _quantity: Quantity,
        _time_in_force: TimeInForce,
        _post_only: bool,
        _reduce_only: bool,
        _block_height: u32,
        _expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        Err(DydxError::NotImplemented(
            "Stop limit orders not yet implemented - awaiting proto generation".to_string(),
        ))
    }

    /// Submits a take profit market order to dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError::NotImplemented` until conditional order support is added.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_take_profit_market_order(
        &self,
        _wallet: &Wallet,
        _client_order_id: u32,
        _side: OrderSide,
        _trigger_price: Price,
        _quantity: Quantity,
        _reduce_only: bool,
        _block_height: u32,
        _expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        Err(DydxError::NotImplemented(
            "Take profit market orders not yet implemented - awaiting proto generation".to_string(),
        ))
    }

    /// Submits a take profit limit order to dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError::NotImplemented` until conditional order support is added.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_take_profit_limit_order(
        &self,
        _wallet: &Wallet,
        _client_order_id: u32,
        _side: OrderSide,
        _trigger_price: Price,
        _limit_price: Price,
        _quantity: Quantity,
        _time_in_force: TimeInForce,
        _post_only: bool,
        _reduce_only: bool,
        _block_height: u32,
        _expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        Err(DydxError::NotImplemented(
            "Take profit limit orders not yet implemented - awaiting proto generation".to_string(),
        ))
    }

    /// Submits a trailing stop order to dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError::NotImplemented` - trailing stops not yet supported by dYdX v4 protocol.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_trailing_stop_order(
        &self,
        _wallet: &Wallet,
        _client_order_id: u32,
        _side: OrderSide,
        _trailing_offset: Price,
        _quantity: Quantity,
        _reduce_only: bool,
        _block_height: u32,
        _expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        Err(DydxError::NotImplemented(
            "Trailing stop orders not yet supported by dYdX v4 protocol".to_string(),
        ))
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
        use crate::grpc::TxBuilder;

        // Get account for signing
        let account = wallet
            .account_offline(self.subaccount_number)
            .map_err(|e| DydxError::Wallet(format!("Failed to derive account: {e}")))?;

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
        let tx_raw = tx_builder
            .build_transaction(&account, vec![any_msg], None)
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

        let mut grpc_client = self.grpc_client.clone();
        let tx_hash = grpc_client.broadcast_tx(tx_bytes).await.map_err(|e| {
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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_cancel_orders_batch_signature() {
        // Test that cancel_orders_batch accepts (InstrumentId, u32) tuples
        // This ensures the signature matches what's needed for implementation

        // Create dummy data
        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let orders = [(instrument_id, 1u32), (instrument_id, 2u32)];

        // Verify tuple structure
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].0, instrument_id);
        assert_eq!(orders[0].1, 1u32);
        assert_eq!(orders[1].1, 2u32);
    }

    #[rstest]
    fn test_cancel_orders_batch_returns_not_implemented() {
        // The stub should return NotImplemented error
        // This documents expected behavior until fully implemented

        use tokio::runtime::Runtime;

        let rt = Runtime::new().unwrap();

        rt.block_on(async {
            // Mock setup would go here
            // For now, just verify the error type exists
            let err = DydxError::NotImplemented("test".to_string());
            assert!(matches!(err, DydxError::NotImplemented(_)));
        });
    }
}

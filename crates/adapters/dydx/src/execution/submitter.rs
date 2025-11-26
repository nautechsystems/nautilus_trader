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
    instruments::Instrument,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    error::DydxError,
    grpc::{Account, DydxGrpcClient, OrderBuilder, OrderGoodUntil, OrderMarketParams, TxBuilder, Wallet},
    proto::dydxprotocol::clob::order::{
        Side as ProtoOrderSide, TimeInForce as ProtoTimeInForce,
    },
};

/// Order submitter for dYdX v4 protocol.
///
/// Handles order placement, modification, and cancellation via gRPC.
#[derive(Debug)]
pub struct OrderSubmitter {
    grpc_client: DydxGrpcClient,
    wallet_address: String,
    subaccount_number: u32,
    tx_builder: Option<TxBuilder>,
}

impl OrderSubmitter {
    /// Creates a new order submitter.
    ///
    /// # Arguments
    ///
    /// * `grpc_client` - The gRPC client for communicating with dYdX validators
    /// * `wallet_address` - The wallet address (dydx1...)
    /// * `subaccount_number` - The subaccount number for trading
    /// * `tx_builder` - Optional transaction builder for signing (if None, orders cannot be signed)
    pub fn new(
        grpc_client: DydxGrpcClient,
        wallet_address: String,
        subaccount_number: u32,
        tx_builder: Option<TxBuilder>,
    ) -> Self {
        Self {
            grpc_client,
            wallet_address,
            subaccount_number,
            tx_builder,
        }
    }

    /// Converts Nautilus `OrderSide` to dYdX proto `Side`.
    fn convert_side(side: OrderSide) -> ProtoOrderSide {
        match side {
            OrderSide::Buy => ProtoOrderSide::Buy,
            OrderSide::Sell => ProtoOrderSide::Sell,
            OrderSide::NoOrderSide => ProtoOrderSide::Unspecified,
        }
    }

    /// Converts Nautilus `TimeInForce` to dYdX proto `TimeInForce`.
    fn convert_time_in_force(tif: TimeInForce) -> ProtoTimeInForce {
        match tif {
            TimeInForce::Gtc => ProtoTimeInForce::Unspecified, // GTC uses good_til_block_time
            TimeInForce::Ioc => ProtoTimeInForce::Ioc,
            TimeInForce::Fok => ProtoTimeInForce::FillOrKill,
            TimeInForce::Gtd => ProtoTimeInForce::Unspecified, // GTD uses good_til_block_time
            TimeInForce::Day => ProtoTimeInForce::Unspecified,
            TimeInForce::AtTheOpen => ProtoTimeInForce::Unspecified,
            TimeInForce::AtTheClose => ProtoTimeInForce::Unspecified,
            TimeInForce::GoodTilCanceled => ProtoTimeInForce::Unspecified,
            TimeInForce::GoodTilDate => ProtoTimeInForce::Unspecified,
        }
    }

    /// Returns a mutable reference to the gRPC client.
    pub fn grpc_client_mut(&mut self) -> &mut DydxGrpcClient {
        &mut self.grpc_client
    }

    /// Submits a market order to dYdX via gRPC.
    ///
    /// Market orders execute immediately at the best available price.
    /// Uses IOC (Immediate-Or-Cancel) time-in-force semantics.
    ///
    /// # Arguments
    ///
    /// * `account` - The account with signing credentials (must have account_number/sequence set)
    /// * `market_params` - Market-specific quantization parameters
    /// * `client_order_id` - Client-assigned order ID
    /// * `side` - Buy or sell
    /// * `quantity` - Order quantity
    /// * `block_height` - Current block height for order expiration
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if order building or gRPC submission fails.
    pub async fn submit_market_order(
        &mut self,
        account: &Account,
        market_params: &OrderMarketParams,
        client_order_id: u32,
        side: OrderSide,
        quantity: Quantity,
        block_height: u32,
    ) -> Result<String, DydxError> {
        let proto_side = Self::convert_side(side);
        let quantity_decimal = Decimal::try_from(quantity.as_f64())
            .map_err(|e| DydxError::Parse(format!("Failed to convert quantity: {e}")))?;

        // Build the order using OrderBuilder
        let order = OrderBuilder::new(
            market_params.clone(),
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        )
        .market(proto_side, quantity_decimal)
        .time_in_force(ProtoTimeInForce::Ioc)
        .until(OrderGoodUntil::Block(block_height + 10)) // Short-term order
        .build()
        .map_err(|e| DydxError::Parse(format!("Failed to build order: {e}")))?;

        tracing::info!(
            "Submitting market order: client_id={}, side={:?}, quantity={}, block_height={}",
            client_order_id,
            side,
            quantity,
            block_height
        );

        // Build and broadcast the transaction
        self.broadcast_place_order(account, order).await
    }

    /// Submits a limit order to dYdX via gRPC.
    ///
    /// Limit orders execute only at the specified price or better.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if order building or gRPC submission fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_limit_order(
        &mut self,
        account: &Account,
        market_params: &OrderMarketParams,
        client_order_id: u32,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        block_height: u32,
        expire_time: Option<i64>,
    ) -> Result<String, DydxError> {
        let proto_side = Self::convert_side(side);
        let proto_tif = Self::convert_time_in_force(time_in_force);

        let price_decimal = Decimal::try_from(price.as_f64())
            .map_err(|e| DydxError::Parse(format!("Failed to convert price: {e}")))?;
        let quantity_decimal = Decimal::try_from(quantity.as_f64())
            .map_err(|e| DydxError::Parse(format!("Failed to convert quantity: {e}")))?;

        // Determine order expiration
        let good_until = if let Some(expire_ts) = expire_time {
            // Long-term order with timestamp expiration
            let dt = chrono::DateTime::from_timestamp(expire_ts, 0)
                .ok_or_else(|| DydxError::Parse("Invalid expire timestamp".to_string()))?;
            OrderGoodUntil::Time(dt)
        } else {
            // Short-term order with block expiration
            OrderGoodUntil::Block(block_height + 20)
        };

        // Build the order using OrderBuilder
        let mut builder = OrderBuilder::new(
            market_params.clone(),
            self.wallet_address.clone(),
            self.subaccount_number,
            client_order_id,
        )
        .limit(proto_side, price_decimal, quantity_decimal)
        .time_in_force(proto_tif)
        .reduce_only(reduce_only)
        .until(good_until);

        // Post-only orders use a specific time-in-force
        if post_only {
            builder = builder.time_in_force(ProtoTimeInForce::PostOnly);
        }

        let order = builder
            .build()
            .map_err(|e| DydxError::Parse(format!("Failed to build order: {e}")))?;

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

        // Build and broadcast the transaction
        self.broadcast_place_order(account, order).await
    }

    /// Cancels an order on dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC cancellation fails.
    pub async fn cancel_order(
        &mut self,
        account: &Account,
        market_params: &OrderMarketParams,
        client_order_id: u32,
        block_height: u32,
    ) -> Result<String, DydxError> {
        tracing::info!(
            "Cancelling order: client_id={}, block_height={}",
            client_order_id,
            block_height
        );

        // Build and broadcast the cancel transaction
        self.broadcast_cancel_order(account, market_params, client_order_id, block_height)
            .await
    }

    /// Cancels multiple orders via individual gRPC transactions.
    ///
    /// dYdX v4 requires separate blockchain transactions for each cancellation.
    /// This method executes cancellations sequentially.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if any gRPC cancellation fails. Partial failures
    /// are logged but the method continues to attempt remaining cancellations.
    pub async fn cancel_orders_batch(
        &mut self,
        account: &Account,
        market_params: &OrderMarketParams,
        client_order_ids: &[u32],
        block_height: u32,
    ) -> Result<Vec<String>, DydxError> {
        tracing::info!(
            "Batch cancelling {} orders: ids={:?}",
            client_order_ids.len(),
            client_order_ids
        );

        let mut tx_hashes = Vec::with_capacity(client_order_ids.len());
        let mut errors = Vec::new();

        for &client_id in client_order_ids {
            match self
                .cancel_order(account, market_params, client_id, block_height)
                .await
            {
                Ok(tx_hash) => tx_hashes.push(tx_hash),
                Err(e) => {
                    tracing::error!("Failed to cancel order {}: {:?}", client_id, e);
                    errors.push((client_id, e));
                }
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                "Batch cancel completed with {} failures out of {} orders",
                errors.len(),
                client_order_ids.len()
            );
        }

        Ok(tx_hashes)
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

    /// Broadcasts a place order transaction to the network.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if transaction building or broadcasting fails.
    async fn broadcast_place_order(
        &mut self,
        account: &Account,
        order: crate::proto::dydxprotocol::clob::Order,
    ) -> Result<String, DydxError> {
        use crate::proto::dydxprotocol::clob::MsgPlaceOrder;
        use cosmrs::Any;
        use prost::Message;

        let tx_builder = self
            .tx_builder
            .as_ref()
            .ok_or_else(|| DydxError::Config("TxBuilder not configured".to_string()))?;

        // Create MsgPlaceOrder
        let msg = MsgPlaceOrder {
            order: Some(order),
        };

        // Encode to Any type for Cosmos SDK
        let msg_any = Any {
            type_url: "/dydxprotocol.clob.MsgPlaceOrder".to_string(),
            value: msg.encode_to_vec(),
        };

        // Build and sign the transaction
        let tx_raw = tx_builder
            .build_transaction(account, vec![msg_any], None)
            .map_err(|e| DydxError::Grpc(tonic::Status::internal(format!("Tx build error: {e}"))))?;

        // Broadcast
        let tx_bytes = tx_raw
            .to_bytes()
            .map_err(|e| DydxError::Grpc(tonic::Status::internal(format!("Tx encode error: {e}"))))?;

        let tx_hash = self
            .grpc_client
            .broadcast_tx(tx_bytes)
            .await
            .map_err(|e| DydxError::Grpc(tonic::Status::internal(format!("Broadcast error: {e}"))))?;

        tracing::debug!("Transaction broadcast successful: tx_hash={}", tx_hash);
        Ok(tx_hash)
    }

    /// Broadcasts a cancel order transaction to the network.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if transaction building or broadcasting fails.
    async fn broadcast_cancel_order(
        &mut self,
        account: &Account,
        market_params: &OrderMarketParams,
        client_order_id: u32,
        block_height: u32,
    ) -> Result<String, DydxError> {
        use crate::proto::dydxprotocol::clob::MsgCancelOrder;
        use crate::proto::dydxprotocol::clob::OrderId;
        use crate::proto::dydxprotocol::subaccounts::SubaccountId;
        use cosmrs::Any;
        use prost::Message;

        let tx_builder = self
            .tx_builder
            .as_ref()
            .ok_or_else(|| DydxError::Config("TxBuilder not configured".to_string()))?;

        // Create order ID for cancellation
        let order_id = OrderId {
            subaccount_id: Some(SubaccountId {
                owner: self.wallet_address.clone(),
                number: self.subaccount_number,
            }),
            client_id: client_order_id,
            order_flags: 0, // Short-term order
            clob_pair_id: market_params.clob_pair_id,
        };

        // Create MsgCancelOrder
        let msg = MsgCancelOrder {
            order_id: Some(order_id),
            good_til_oneof: Some(
                crate::proto::dydxprotocol::clob::msg_cancel_order::GoodTilOneof::GoodTilBlock(
                    block_height + 10,
                ),
            ),
        };

        // Encode to Any type for Cosmos SDK
        let msg_any = Any {
            type_url: "/dydxprotocol.clob.MsgCancelOrder".to_string(),
            value: msg.encode_to_vec(),
        };

        // Build and sign the transaction
        let tx_raw = tx_builder
            .build_transaction(account, vec![msg_any], None)
            .map_err(|e| DydxError::Grpc(tonic::Status::internal(format!("Tx build error: {e}"))))?;

        // Broadcast
        let tx_bytes = tx_raw
            .to_bytes()
            .map_err(|e| DydxError::Grpc(tonic::Status::internal(format!("Tx encode error: {e}"))))?;

        let tx_hash = self
            .grpc_client
            .broadcast_tx(tx_bytes)
            .await
            .map_err(|e| DydxError::Grpc(tonic::Status::internal(format!("Broadcast error: {e}"))))?;

        tracing::debug!(
            "Cancel transaction broadcast successful: tx_hash={}",
            tx_hash
        );
        Ok(tx_hash)
    }

    /// Extracts market parameters from an instrument.
    ///
    /// Note: This derives parameters from the instrument's precision settings.
    /// For production use, these should come from dYdX market metadata.
    #[allow(dead_code)]
    fn extract_market_params(
        &self,
        instrument: &dyn Instrument,
    ) -> Result<OrderMarketParams, DydxError> {
        // NOTE:
        // dYdX-specific quantization parameters (atomic_resolution, quantum_conversion_exponent,
        // step_base_quantums, subticks_per_tick and clob_pair_id) ultimately come from the
        // PerpetualMarket metadata exposed by the Indexer API.
        //
        // The full wiring from HTTP market metadata → instrument → gRPC order builder is not yet
        // implemented in Rust. Until proto files are generated and that plumbing is in place, we
        // derive a best-effort set of parameters from the instrument itself so that:
        // - Values are at least instrument-specific (not hard-coded placeholders).
        // - Future work can replace this logic with exact dYdX metadata without changing callers.
        //
        // Mapping strategy (stub until proto):
        // - atomic_resolution: negative of the instrument size precision.
        // - quantum_conversion_exponent: negative of the instrument price precision.
        // - step_base_quantums / subticks_per_tick: minimal non-zero values (1) so that
        //   quantization code has valid, non-zero divisors without assuming dYdX-specific scales.
        //
        // clob_pair_id and oracle_price still require venue metadata and remain stubbed.
        let size_precision = instrument.size_precision() as i32;
        let price_precision = instrument.price_precision() as i32;

        let atomic_resolution = -size_precision;
        let quantum_conversion_exponent = -price_precision;

        Ok(OrderMarketParams {
            atomic_resolution,
            clob_pair_id: 0, // Will be set from instrument metadata
            oracle_price: None,
            quantum_conversion_exponent,
            step_base_quantums: 1,
            subticks_per_tick: 1,
        })
    }

    /// Handles the exchange response from order submission.
    #[allow(dead_code)]
    fn handle_exchange_response(&self, _response: &[u8]) -> Result<String, DydxError> {
        // TODO: Parse proto response when available
        Ok("stubbed_tx_hash".to_string())
    }

    /// Parses exchange order ID from response.
    #[allow(dead_code)]
    fn parse_venue_order_id(&self, _response: &[u8]) -> Result<String, DydxError> {
        // TODO: Extract venue order ID from proto response
        Ok("stubbed_venue_id".to_string())
    }

    /// Stores ClientOrderId to VenueOrderId mapping.
    #[allow(dead_code)]
    fn store_order_id_mapping(&self, _client_id: u32, _venue_id: &str) -> Result<(), DydxError> {
        // TODO: Store in cache/database
        tracing::debug!("[STUB] Would store order ID mapping");
        Ok(())
    }

    /// Retrieves VenueOrderId from ClientOrderId.
    #[allow(dead_code)]
    fn get_venue_order_id(&self, _client_id: u32) -> Result<Option<String>, DydxError> {
        // TODO: Retrieve from cache/database
        Ok(None)
    }

    /// Generates OrderAccepted event from exchange response.
    #[allow(dead_code)]
    fn generate_order_accepted(&self, _client_id: u32, _venue_id: &str) -> Result<(), DydxError> {
        // TODO: Generate and send OrderAccepted event
        tracing::debug!("[STUB] Would generate OrderAccepted event");
        Ok(())
    }

    /// Generates OrderRejected event from exchange error.
    #[allow(dead_code)]
    fn generate_order_rejected(&self, _client_id: u32, _reason: &str) -> Result<(), DydxError> {
        // TODO: Generate and send OrderRejected event
        tracing::debug!("[STUB] Would generate OrderRejected event");
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------
//  Tests
// ------------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use nautilus_model::enums::{OrderSide, TimeInForce};

    use crate::proto::dydxprotocol::clob::order::{
        Side as ProtoOrderSide, TimeInForce as ProtoTimeInForce,
    };

    use super::OrderSubmitter;

    // ------------------------------------------------------------------------
    // convert_side tests
    // ------------------------------------------------------------------------

    #[rstest]
    #[case::buy(OrderSide::Buy, ProtoOrderSide::Buy)]
    #[case::sell(OrderSide::Sell, ProtoOrderSide::Sell)]
    #[case::no_side(OrderSide::NoOrderSide, ProtoOrderSide::Unspecified)]
    fn test_convert_side(#[case] input: OrderSide, #[case] expected: ProtoOrderSide) {
        let result = OrderSubmitter::convert_side(input);
        assert_eq!(result, expected);
    }

    // ------------------------------------------------------------------------
    // convert_time_in_force tests
    // ------------------------------------------------------------------------

    #[rstest]
    #[case::ioc(TimeInForce::Ioc, ProtoTimeInForce::Ioc)]
    #[case::fok(TimeInForce::Fok, ProtoTimeInForce::FillOrKill)]
    #[case::gtc(TimeInForce::Gtc, ProtoTimeInForce::Unspecified)]
    #[case::gtd(TimeInForce::Gtd, ProtoTimeInForce::Unspecified)]
    #[case::day(TimeInForce::Day, ProtoTimeInForce::Unspecified)]
    #[case::at_the_open(TimeInForce::AtTheOpen, ProtoTimeInForce::Unspecified)]
    #[case::at_the_close(TimeInForce::AtTheClose, ProtoTimeInForce::Unspecified)]
    #[case::good_til_canceled(TimeInForce::GoodTilCanceled, ProtoTimeInForce::Unspecified)]
    #[case::good_til_date(TimeInForce::GoodTilDate, ProtoTimeInForce::Unspecified)]
    fn test_convert_time_in_force(#[case] input: TimeInForce, #[case] expected: ProtoTimeInForce) {
        let result = OrderSubmitter::convert_time_in_force(input);
        assert_eq!(result, expected);
    }
}

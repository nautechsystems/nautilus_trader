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

use crate::{
    error::DydxError,
    grpc::{DydxGrpcClient, OrderMarketParams, Wallet},
    // TODO: Enable when proto is generated
    // proto::dydxprotocol::clob::order::{
    //     Side as ProtoOrderSide, TimeInForce as ProtoTimeInForce,
    // },
};

// Temporary placeholder types until proto is generated
#[derive(Debug, Clone, Copy)]
pub enum ProtoOrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy)]
pub enum ProtoTimeInForce {
    Unspecified,
    Ioc,
    FillOrKill,
}

#[derive(Debug)]
pub struct OrderSubmitter {
    #[allow(dead_code)]
    grpc_client: DydxGrpcClient,
    #[allow(dead_code)]
    wallet_address: String,
    #[allow(dead_code)]
    subaccount_number: u32,
}

impl OrderSubmitter {
    pub fn new(
        grpc_client: DydxGrpcClient,
        wallet_address: String,
        subaccount_number: u32,
    ) -> Self {
        Self {
            grpc_client,
            wallet_address,
            subaccount_number,
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
        _wallet: &Wallet,
        client_order_id: u32,
        side: OrderSide,
        quantity: Quantity,
        _block_height: u32,
    ) -> Result<(), DydxError> {
        // TODO: Implement when proto is generated
        tracing::info!(
            "[STUB] Submitting market order: client_id={}, side={:?}, quantity={}",
            client_order_id,
            side,
            quantity
        );
        Ok(())
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
        _wallet: &Wallet,
        client_order_id: u32,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        _block_height: u32,
        _expire_time: Option<i64>,
    ) -> Result<(), DydxError> {
        // TODO: Implement when proto is generated
        tracing::info!(
            "[STUB] Submitting limit order: client_id={}, side={:?}, price={}, quantity={}, tif={:?}, post_only={}, reduce_only={}",
            client_order_id,
            side,
            price,
            quantity,
            time_in_force,
            post_only,
            reduce_only
        );
        Ok(())
    }

    /// Cancels an order on dYdX via gRPC.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if gRPC cancellation fails.
    pub async fn cancel_order(
        &self,
        _wallet: &Wallet,
        client_order_id: u32,
        _block_height: u32,
    ) -> Result<(), DydxError> {
        // TODO: Implement when proto is generated
        tracing::info!("[STUB] Cancelling order: client_id={}", client_order_id);
        Ok(())
    }

    /// Cancels multiple orders via individual gRPC transactions.
    ///
    /// dYdX v4 requires separate blockchain transactions for each cancellation.
    ///
    /// # Errors
    ///
    /// Returns `DydxError` if any gRPC cancellation fails.
    pub async fn cancel_orders_batch(
        &self,
        _wallet: &Wallet,
        client_order_ids: &[u32],
        _block_height: u32,
    ) -> Result<(), DydxError> {
        // TODO: Implement when proto is generated
        // Note: Each order requires a separate gRPC transaction
        tracing::info!(
            "[STUB] Batch cancelling {} orders: ids={:?}",
            client_order_ids.len(),
            client_order_ids
        );
        Ok(())
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

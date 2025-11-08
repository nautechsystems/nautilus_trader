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

    /// Submits a market order to dYdX.
    ///
    /// # Errors
    ///
    /// Returns an error if order submission fails. Currently stubbed.
    pub async fn submit_market_order(
        &self,
        _wallet: &Wallet,
        _instrument: &dyn Instrument,
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

    /// Submits a limit order to dYdX.
    ///
    /// # Errors
    ///
    /// Returns an error if order submission fails. Currently stubbed.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_limit_order(
        &self,
        _wallet: &Wallet,
        _instrument: &dyn Instrument,
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

    #[allow(dead_code)]
    fn extract_market_params(
        &self,
        _instrument: &dyn Instrument,
    ) -> Result<OrderMarketParams, DydxError> {
        // TODO: Extract from instrument once we have proper metadata handling
        // For now, return placeholder values
        Ok(OrderMarketParams {
            atomic_resolution: -10,
            clob_pair_id: 0, // Will be set from instrument metadata
            oracle_price: None,
            quantum_conversion_exponent: -9,
            step_base_quantums: 1_000_000,
            subticks_per_tick: 100_000,
        })
    }
}

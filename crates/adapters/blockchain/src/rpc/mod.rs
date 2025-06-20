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

//! RPC client implementations for blockchain network communication.
//!
//! This module provides JSON-RPC client implementations for communicating with various
//! blockchain networks via HTTP and WebSocket connections. It includes specialized
//! clients for different networks (Ethereum, Polygon, Arbitrum, Base) and common
//! utilities for handling RPC requests and responses.

use enum_dispatch::enum_dispatch;

use crate::rpc::{
    chains::{
        arbitrum::ArbitrumRpcClient, base::BaseRpcClient, ethereum::EthereumRpcClient,
        polygon::PolygonRpcClient,
    },
    error::BlockchainRpcClientError,
    types::BlockchainMessage,
};

pub mod chains;
pub mod core;
pub mod error;
pub mod http;
pub mod types;
pub mod utils;

#[enum_dispatch(BlockchainRpcClient)]
#[derive(Debug)]
pub enum BlockchainRpcClientAny {
    Arbitrum(ArbitrumRpcClient),
    Base(BaseRpcClient),
    Ethereum(EthereumRpcClient),
    Polygon(PolygonRpcClient),
}

#[async_trait::async_trait]
#[enum_dispatch]
pub trait BlockchainRpcClient {
    async fn connect(&mut self) -> anyhow::Result<()>;
    async fn subscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError>;
    async fn subscribe_swaps(&mut self) -> Result<(), BlockchainRpcClientError> {
        todo!("Not implemented")
    }
    async fn unsubscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError>;
    async fn unsubscribe_swaps(&mut self) -> Result<(), BlockchainRpcClientError> {
        todo!("Not implemented")
    }
    async fn next_rpc_message(&mut self) -> Result<BlockchainMessage, BlockchainRpcClientError>;
}

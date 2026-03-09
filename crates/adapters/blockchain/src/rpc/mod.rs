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
pub mod helpers;
pub mod http;
pub mod providers;
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

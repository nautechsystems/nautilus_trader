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

//! DeFi (Decentralized Finance) domain model.
//!
//! This module gathers all constructs required to model on-chain markets and decentralised
//! exchange (DEX) activity.
//!
//! • `chain`    – Blockchain networks supported by Nautilus (Ethereum, Arbitrum, …).
//! • `token`    – ERC-20 and other fungible token metadata.
//! • `dex`      – DEX protocol definitions (Uniswap V3, PancakeSwap, …).
//! • `data`     – Domain events & state snapshots that flow through the system (Block, PoolSwap).
//! • `types`    – Numeric value types (Money, Quantity, Price) shared across the DeFi layer.
//! • `rpc`      – Lightweight JSON-RPC helpers used by on-chain adapters.

pub mod amm;
pub mod chain;
pub mod data;
pub mod dex;
pub mod hex;
pub mod pool_analysis;
pub mod reporting;
pub mod rpc;
pub mod tick_map;
pub mod token;
pub mod types;
pub mod validation;

// Re-exports
pub use amm::{Pool, SharedPool};
pub use chain::{Blockchain, Chain, SharedChain};
pub use data::{
    DefiData,
    block::Block,
    collect::PoolFeeCollect,
    flash::PoolFlash,
    liquidity::{PoolLiquidityUpdate, PoolLiquidityUpdateType},
    swap::PoolSwap,
    transaction::Transaction,
};
pub use dex::{AmmType, Dex, DexType, SharedDex};
pub use pool_analysis::PoolProfiler;
pub use token::{SharedToken, Token};

/// Number of decimal places used by the native Ether denomination.
///
/// On the EVM all ERC-20 balances are expressed in **wei** – the
/// smallest indivisible unit of Ether, named after cryptographer
/// Wei Dai. One ether equals `10^18` wei, so using 18 decimal
/// places is sufficient to represent any value expressible on-chain.
///
/// Tokens that choose a smaller precision (e.g. USDC’s 6, WBTC’s 8)
/// still fall below this upper bound.
pub static WEI_PRECISION: u8 = 18;

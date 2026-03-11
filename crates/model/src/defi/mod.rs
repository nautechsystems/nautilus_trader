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
pub mod pool_identifier;
pub mod reporting;
pub mod rpc;
pub mod tick_map;
pub mod token;
pub mod tx_hash;
pub mod types;
pub mod validation;
pub mod wallet;

#[cfg(test)]
pub mod stubs;

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
    transaction_receipt::{ReceiptLog, TransactionReceipt},
};
pub use dex::{AmmType, Dex, DexType, SharedDex};
pub use pool_analysis::PoolProfiler;
pub use pool_identifier::PoolIdentifier;
pub use token::{SharedToken, Token};
pub use tx_hash::{decode_raw_tx_hex, tx_hash_from_raw_tx_hex, tx_hash_hex_from_raw_tx_hex};

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

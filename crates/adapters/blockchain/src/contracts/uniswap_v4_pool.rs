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

//! Uniswap v4 PoolManager contract bindings.
//!
//! V4 uses a singleton PoolManager architecture instead of per-pool contracts.
//! All pool state is stored in the PoolManager and identified by a PoolKey.
//! Hooks provide customizable execution logic before/after swaps and liquidity modifications.

use std::sync::Arc;

use alloy::{
    primitives::{Address, FixedBytes, U160, U256, keccak256},
    sol,
    sol_types::SolCall,
};
use thiserror::Error;

use super::base::{BaseContract, ContractCall};
use crate::rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient};

sol! {
    /// Uniswap v4 PoolManager interface
    ///
    /// The singleton contract that manages all v4 pools. Unlike v3 where each pool
    /// is a separate contract, v4 stores all pool state in this single contract.
    #[sol(rpc)]
    contract UniswapV4PoolManager {
        // ========== View Functions ==========

        /// Get the slot0 state for a pool
        function getSlot0(bytes32 poolId) external view returns (
            uint160 sqrtPriceX96,
            int24 tick,
            uint24 protocolFee,
            uint24 lpFee
        );

        /// Get the liquidity for a pool
        function getLiquidity(bytes32 poolId) external view returns (uint128 liquidity);

        /// Get tick info for a specific tick
        function getTickInfo(bytes32 poolId, int24 tick) external view returns (
            uint128 liquidityGross,
            int128 liquidityNet,
            uint256 feeGrowthOutside0X128,
            uint256 feeGrowthOutside1X128
        );

        /// Get position info
        function getPositionInfo(
            bytes32 poolId,
            address owner,
            int24 tickLower,
            int24 tickUpper,
            bytes32 salt
        ) external view returns (
            uint128 liquidity,
            uint256 feeGrowthInside0LastX128,
            uint256 feeGrowthInside1LastX128
        );

        /// Get fee growth globals for a pool
        function getFeeGrowthGlobals(bytes32 poolId) external view returns (
            uint256 feeGrowthGlobal0X128,
            uint256 feeGrowthGlobal1X128
        );
    }
}

/// Pool state data for v4 pools
#[derive(Debug, Clone)]
pub struct UniswapV4PoolState {
    /// The current sqrt price (Q64.96 format)
    pub sqrt_price_x96: U160,
    /// The current tick
    pub tick: i32,
    /// The protocol fee (0-100%)
    pub protocol_fee: u32,
    /// The LP fee in hundredths of a bip
    pub lp_fee: u32,
    /// The current liquidity
    pub liquidity: u128,
    /// Fee growth global for token0
    pub fee_growth_global_0_x128: U256,
    /// Fee growth global for token1
    pub fee_growth_global_1_x128: U256,
}

/// Tick data for v4 pools
#[derive(Debug, Clone, Default)]
pub struct UniswapV4TickData {
    /// Total liquidity referencing this tick
    pub liquidity_gross: u128,
    /// Net liquidity change when crossing this tick
    pub liquidity_net: i128,
    /// Fee growth outside for token0
    pub fee_growth_outside_0_x128: U256,
    /// Fee growth outside for token1
    pub fee_growth_outside_1_x128: U256,
}

/// Position data for v4 pools
#[derive(Debug, Clone, Default)]
pub struct UniswapV4PositionData {
    /// The liquidity of the position
    pub liquidity: u128,
    /// Fee growth inside for token0 at last update
    pub fee_growth_inside_0_last_x128: U256,
    /// Fee growth inside for token1 at last update
    pub fee_growth_inside_1_last_x128: U256,
}

/// Complete pool snapshot for v4
#[derive(Debug, Clone)]
pub struct UniswapV4PoolSnapshot {
    /// Pool identifier (keccak256 of PoolKey)
    pub pool_id: FixedBytes<32>,
    /// Currency0 address (lower address)
    pub currency0: Address,
    /// Currency1 address (higher address)
    pub currency1: Address,
    /// Pool fee in hundredths of a bip
    pub fee: u32,
    /// Tick spacing
    pub tick_spacing: i32,
    /// Hooks contract address
    pub hooks: Address,
    /// Current pool state
    pub state: UniswapV4PoolState,
    /// Tick data (populated for requested ticks)
    pub ticks: Vec<(i32, UniswapV4TickData)>,
    /// Position data (populated for requested positions)
    pub positions: Vec<UniswapV4PositionData>,
}

/// Error types for v4 pool operations
#[derive(Debug, Error)]
pub enum UniswapV4PoolError {
    #[error("Pool not initialized")]
    PoolNotInitialized,
    #[error("RPC error: {0}")]
    RpcError(#[from] BlockchainRpcClientError),
    #[error("Invalid pool key")]
    InvalidPoolKey,
    #[error("Contract call failed: {0}")]
    ContractError(String),
    #[error("Failed to decode {field} for pool {pool_id}: {reason}")]
    DecodingError {
        field: String,
        pool_id: String,
        reason: String,
    },
}

/// Uniswap v4 PoolManager contract wrapper
///
/// Provides methods to query pool state from the v4 singleton PoolManager contract.
#[derive(Debug)]
pub struct UniswapV4PoolManagerContract {
    /// The base contract providing common RPC execution functionality.
    base: BaseContract,
    /// The PoolManager contract address.
    pool_manager_address: Address,
}

impl UniswapV4PoolManagerContract {
    /// Creates a new v4 PoolManager contract wrapper.
    #[must_use]
    pub fn new(pool_manager_address: Address, client: Arc<BlockchainHttpRpcClient>) -> Self {
        Self {
            base: BaseContract::new(client),
            pool_manager_address,
        }
    }

    /// Get the PoolManager contract address.
    #[must_use]
    pub const fn address(&self) -> &Address {
        &self.pool_manager_address
    }

    /// Compute the pool ID from a PoolKey.
    ///
    /// The pool ID is keccak256(abi.encode(currency0, currency1, fee, tickSpacing, hooks))
    #[must_use]
    pub fn compute_pool_id(
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
    ) -> FixedBytes<32> {
        // ABI encode the PoolKey struct (each field padded to 32 bytes)
        let mut data = Vec::with_capacity(160);

        // currency0 (address, padded to 32 bytes)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(currency0.as_slice());

        // currency1 (address, padded to 32 bytes)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(currency1.as_slice());

        // fee (uint24, padded to 32 bytes)
        let mut fee_bytes = [0u8; 32];
        fee_bytes[29..32].copy_from_slice(&fee.to_be_bytes()[1..4]);
        data.extend_from_slice(&fee_bytes);

        // tickSpacing (int24, sign-extended to 32 bytes)
        let tick_bytes = if tick_spacing >= 0 {
            let mut bytes = [0u8; 32];
            let ts_bytes = (tick_spacing as u32).to_be_bytes();
            bytes[29..32].copy_from_slice(&ts_bytes[1..4]);
            bytes
        } else {
            let mut bytes = [0xffu8; 32];
            let ts_bytes = tick_spacing.to_be_bytes();
            bytes[29..32].copy_from_slice(&ts_bytes[1..4]);
            bytes
        };
        data.extend_from_slice(&tick_bytes);

        // hooks (address, padded to 32 bytes)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(hooks.as_slice());

        keccak256(&data)
    }

    /// Get the current state of a pool.
    ///
    /// # Errors
    ///
    /// Returns error if the RPC call fails or pool doesn't exist.
    pub async fn get_pool_state(
        &self,
        pool_id: FixedBytes<32>,
        block: Option<u64>,
    ) -> Result<UniswapV4PoolState, UniswapV4PoolError> {
        // Build multicall for all state queries
        let calls = vec![
            ContractCall {
                target: self.pool_manager_address,
                allow_failure: false,
                call_data: UniswapV4PoolManager::getSlot0Call { poolId: pool_id }.abi_encode(),
            },
            ContractCall {
                target: self.pool_manager_address,
                allow_failure: false,
                call_data: UniswapV4PoolManager::getLiquidityCall { poolId: pool_id }.abi_encode(),
            },
            ContractCall {
                target: self.pool_manager_address,
                allow_failure: false,
                call_data: UniswapV4PoolManager::getFeeGrowthGlobalsCall { poolId: pool_id }
                    .abi_encode(),
            },
        ];

        let results = self.base.execute_multicall(calls, block).await?;

        if results.len() != 3 {
            return Err(UniswapV4PoolError::ContractError(format!(
                "Expected 3 results, got {}",
                results.len()
            )));
        }

        // Decode slot0
        let slot0 = UniswapV4PoolManager::getSlot0Call::abi_decode_returns(&results[0].returnData)
            .map_err(|e| UniswapV4PoolError::DecodingError {
                field: "slot0".to_string(),
                pool_id: hex::encode(pool_id),
                reason: e.to_string(),
            })?;

        // Decode liquidity
        let liquidity =
            UniswapV4PoolManager::getLiquidityCall::abi_decode_returns(&results[1].returnData)
                .map_err(|e| UniswapV4PoolError::DecodingError {
                    field: "liquidity".to_string(),
                    pool_id: hex::encode(pool_id),
                    reason: e.to_string(),
                })?;

        // Decode fee growth
        let fee_growth = UniswapV4PoolManager::getFeeGrowthGlobalsCall::abi_decode_returns(
            &results[2].returnData,
        )
        .map_err(|e| UniswapV4PoolError::DecodingError {
            field: "feeGrowthGlobals".to_string(),
            pool_id: hex::encode(pool_id),
            reason: e.to_string(),
        })?;

        Ok(UniswapV4PoolState {
            sqrt_price_x96: slot0.sqrtPriceX96,
            tick: slot0.tick.as_i32(),
            protocol_fee: slot0.protocolFee.to::<u32>(),
            lp_fee: slot0.lpFee.to::<u32>(),
            liquidity,
            fee_growth_global_0_x128: fee_growth.feeGrowthGlobal0X128,
            fee_growth_global_1_x128: fee_growth.feeGrowthGlobal1X128,
        })
    }

    /// Get tick data for a specific tick.
    ///
    /// # Errors
    ///
    /// Returns error if the RPC call fails.
    pub async fn get_tick(
        &self,
        pool_id: FixedBytes<32>,
        tick: i32,
        block: Option<u64>,
    ) -> Result<UniswapV4TickData, UniswapV4PoolError> {
        use alloy::sol_types::private::primitives::aliases::I24;

        let tick_i24 = I24::try_from(tick).map_err(|_| {
            UniswapV4PoolError::ContractError(format!("Tick {tick} out of range for int24"))
        })?;

        let call_data = UniswapV4PoolManager::getTickInfoCall {
            poolId: pool_id,
            tick: tick_i24,
        }
        .abi_encode();

        let raw_response = self
            .base
            .execute_call(&self.pool_manager_address, &call_data, block)
            .await?;

        let tick_info = UniswapV4PoolManager::getTickInfoCall::abi_decode_returns(&raw_response)
            .map_err(|e| UniswapV4PoolError::DecodingError {
                field: format!("tick({tick})"),
                pool_id: hex::encode(pool_id),
                reason: e.to_string(),
            })?;

        Ok(UniswapV4TickData {
            liquidity_gross: tick_info.liquidityGross,
            liquidity_net: tick_info.liquidityNet,
            fee_growth_outside_0_x128: tick_info.feeGrowthOutside0X128,
            fee_growth_outside_1_x128: tick_info.feeGrowthOutside1X128,
        })
    }

    /// Batch get tick data for multiple ticks using multicall.
    ///
    /// # Errors
    ///
    /// Returns error if the multicall fails.
    pub async fn batch_get_ticks(
        &self,
        pool_id: FixedBytes<32>,
        ticks: &[i32],
        block: Option<u64>,
    ) -> Result<Vec<(i32, UniswapV4TickData)>, UniswapV4PoolError> {
        use alloy::sol_types::private::primitives::aliases::I24;

        if ticks.is_empty() {
            return Ok(Vec::new());
        }

        // Build multicall
        let calls: Vec<ContractCall> = ticks
            .iter()
            .filter_map(|&tick| {
                I24::try_from(tick).ok().map(|tick_i24| ContractCall {
                    target: self.pool_manager_address,
                    allow_failure: true,
                    call_data: UniswapV4PoolManager::getTickInfoCall {
                        poolId: pool_id,
                        tick: tick_i24,
                    }
                    .abi_encode(),
                })
            })
            .collect();

        let results = self.base.execute_multicall(calls, block).await?;

        let mut tick_data = Vec::with_capacity(ticks.len());
        for (i, &tick_value) in ticks.iter().enumerate() {
            if i >= results.len() {
                break;
            }

            let result = &results[i];
            if !result.success {
                continue;
            }

            if let Ok(decoded) =
                UniswapV4PoolManager::getTickInfoCall::abi_decode_returns(&result.returnData)
            {
                tick_data.push((
                    tick_value,
                    UniswapV4TickData {
                        liquidity_gross: decoded.liquidityGross,
                        liquidity_net: decoded.liquidityNet,
                        fee_growth_outside_0_x128: decoded.feeGrowthOutside0X128,
                        fee_growth_outside_1_x128: decoded.feeGrowthOutside1X128,
                    },
                ));
            }
        }

        Ok(tick_data)
    }

    /// Get position data.
    ///
    /// # Errors
    ///
    /// Returns error if the RPC call fails.
    pub async fn get_position(
        &self,
        pool_id: FixedBytes<32>,
        owner: Address,
        tick_lower: i32,
        tick_upper: i32,
        salt: FixedBytes<32>,
        block: Option<u64>,
    ) -> Result<UniswapV4PositionData, UniswapV4PoolError> {
        use alloy::sol_types::private::primitives::aliases::I24;

        let tick_lower_i24 = I24::try_from(tick_lower).map_err(|_| {
            UniswapV4PoolError::ContractError(format!(
                "tick_lower {tick_lower} out of range for int24"
            ))
        })?;

        let tick_upper_i24 = I24::try_from(tick_upper).map_err(|_| {
            UniswapV4PoolError::ContractError(format!(
                "tick_upper {tick_upper} out of range for int24"
            ))
        })?;

        let call_data = UniswapV4PoolManager::getPositionInfoCall {
            poolId: pool_id,
            owner,
            tickLower: tick_lower_i24,
            tickUpper: tick_upper_i24,
            salt,
        }
        .abi_encode();

        let raw_response = self
            .base
            .execute_call(&self.pool_manager_address, &call_data, block)
            .await?;

        let position_info = UniswapV4PoolManager::getPositionInfoCall::abi_decode_returns(
            &raw_response,
        )
        .map_err(|e| UniswapV4PoolError::DecodingError {
            field: "position".to_string(),
            pool_id: hex::encode(pool_id),
            reason: e.to_string(),
        })?;

        Ok(UniswapV4PositionData {
            liquidity: position_info.liquidity,
            fee_growth_inside_0_last_x128: position_info.feeGrowthInside0LastX128,
            fee_growth_inside_1_last_x128: position_info.feeGrowthInside1LastX128,
        })
    }

    /// Fetch a complete snapshot of a pool's state.
    ///
    /// # Errors
    ///
    /// Returns error if any RPC call fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_snapshot(
        &self,
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
        tick_range: Option<(i32, i32)>,
        block: Option<u64>,
    ) -> Result<UniswapV4PoolSnapshot, UniswapV4PoolError> {
        let pool_id = Self::compute_pool_id(currency0, currency1, fee, tick_spacing, hooks);

        // Get pool state
        let state = self.get_pool_state(pool_id, block).await?;

        // Optionally fetch tick data for a range
        let ticks = if let Some((lower, upper)) = tick_range {
            let tick_indices: Vec<i32> = (lower..=upper)
                .step_by(tick_spacing.unsigned_abs() as usize)
                .collect();
            self.batch_get_ticks(pool_id, &tick_indices, block).await?
        } else {
            Vec::new()
        };

        Ok(UniswapV4PoolSnapshot {
            pool_id,
            currency0,
            currency1,
            fee,
            tick_spacing,
            hooks,
            state,
            ticks,
            positions: Vec::new(),
        })
    }
}

/// Known PoolManager deployment addresses by chain ID
/// Reference: https://docs.uniswap.org/contracts/v4/deployments
pub mod deployments {
    use alloy::primitives::{Address, address};

    /// Get the PoolManager address for a given chain ID.
    #[must_use]
    pub const fn pool_manager_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(address!("000000000004444c5dc75cB358380D2e3dE08A90")), // Ethereum
            10 => Some(address!("9a13f98cb987694c9f086b1f5eb990eea8264ec3")), // Optimism
            56 => Some(address!("28e2ea090877bf75740558f6bfb36a5ffee9e9df")), // BNB
            130 => Some(address!("1f98400000000000000000000000000000000004")), // Unichain
            137 => Some(address!("67366782805870060151383f4bbff9dab53e5cd6")), // Polygon
            480 => Some(address!("b1860d529182ac3bc1f51fa2abd56662b7d13f33")), // Worldchain
            1868 => Some(address!("360e68faccca8ca495c1b759fd9eee466db9fb32")), // Soneium
            7777777 => Some(address!("0575338e4c17006ae181b47900a84404247ca30f")), // Zora
            8453 => Some(address!("498581ff718922c3f8e6a244956af099b2652b2b")), // Base
            42161 => Some(address!("360e68faccca8ca495c1b759fd9eee466db9fb32")), // Arbitrum
            42220 => Some(address!("288dc841A52FCA2707c6947B3A777c5E56cd87BC")), // Celo
            43114 => Some(address!("06380c0e0912312b5150364b9dc4542ba0dbbc85")), // Avalanche
            57073 => Some(address!("360e68faccca8ca495c1b759fd9eee466db9fb32")), // Ink
            81457 => Some(address!("1631559198a9e474033433b2958dabc135ab6446")), // Blast
            // Testnets
            1301 => Some(address!("00b036b58a818b1bc34d502d3fe730db729e62ac")), // Unichain Sepolia
            11155111 => Some(address!("E03A1074c86CFeDd5C142C4F04F1a1536e203543")), // Sepolia
            84532 => Some(address!("05E73354cFDd6745C338b50BcFDfA3Aa6fA03408")), // Base Sepolia
            421614 => Some(address!("FB3e0C6F74eB1a21CC1Da29aeC80D2Dfe6C9a317")), // Arbitrum Sepolia
            _ => None,
        }
    }

    /// Get the StateView address for a given chain ID.
    #[must_use]
    pub const fn state_view_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(address!("7ffe42c4a5deea5b0fec41c94c136cf115597227")), // Ethereum
            10 => Some(address!("c18a3169788f4f75a170290584eca6395c75ecdb")), // Optimism
            56 => Some(address!("d13dd3d6e93f276fafc9db9e6bb47c1180aee0c4")), // BNB
            130 => Some(address!("86e8631a016f9068c3f085faf484ee3f5fdee8f2")), // Unichain
            137 => Some(address!("5ea1bd7974c8a611cbab0bdcafcb1d9cc9b3ba5a")), // Polygon
            480 => Some(address!("51d394718bc09297262e368c1a481217fdeb71eb")), // Worldchain
            1868 => Some(address!("76fd297e2d437cd7f76d50f01afe6160f86e9990")), // Soneium
            7777777 => Some(address!("385785af07d63b50d0a0ea57c4ff89d06adf7328")), // Zora
            8453 => Some(address!("a3c0c9b65bad0b08107aa264b0f3db444b867a71")), // Base
            42161 => Some(address!("76fd297e2d437cd7f76d50f01afe6160f86e9990")), // Arbitrum
            42220 => Some(address!("bc21f8720babf4b20d195ee5c6e99c52b76f2bfb")), // Celo
            43114 => Some(address!("c3c9e198c735a4b97e3e683f391ccbdd60b69286")), // Avalanche
            57073 => Some(address!("76fd297e2d437cd7f76d50f01afe6160f86e9990")), // Ink
            81457 => Some(address!("12a88ae16f46dce4e8b15368008ab3380885df30")), // Blast
            // Testnets
            1301 => Some(address!("c199f1072a74d4e905aba1a84d9a45e2546b6222")), // Unichain Sepolia
            11155111 => Some(address!("e1dd9c3fa50edb962e442f60dfbc432e24537e4c")), // Sepolia
            84532 => Some(address!("571291b572ed32ce6751a2cb2486ebee8defb9b4")), // Base Sepolia
            421614 => Some(address!("9d467fa9062b6e9b1a46e26007ad82db116c67cb")), // Arbitrum Sepolia
            _ => None,
        }
    }

    /// Get the Universal Router address for a given chain ID.
    #[must_use]
    pub const fn universal_router_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(address!("66a9893cc07d91d95644aedd05d03f95e1dba8af")), // Ethereum
            10 => Some(address!("851116d9223fabed8e56c0e6b8ad0c31d98b3507")), // Optimism
            56 => Some(address!("1906c1d672b88cd1b9ac7593301ca990f94eae07")), // BNB
            130 => Some(address!("ef740bf23acae26f6492b10de645d6b98dc8eaf3")), // Unichain
            137 => Some(address!("1095692a6237d83c6a72f3f5efedb9a670c49223")), // Polygon
            480 => Some(address!("8ac7bee993bb44dab564ea4bc9ea67bf9eb5e743")), // Worldchain
            1868 => Some(address!("4cded7edf52c8aa5259a54ec6a3ce7c6d2a455df")), // Soneium
            7777777 => Some(address!("3315ef7ca28db74abadc6c44570efdf06b04b020")), // Zora
            8453 => Some(address!("6ff5693b99212da76ad316178a184ab56d299b43")), // Base
            42161 => Some(address!("a51afafe0263b40edaef0df8781ea9aa03e381a3")), // Arbitrum
            42220 => Some(address!("cb695bc5d3aa22cad1e6df07801b061a05a0233a")), // Celo
            43114 => Some(address!("94b75331ae8d42c1b61065089b7d48fe14aa73b7")), // Avalanche
            57073 => Some(address!("112908dac86e20e7241b0927479ea3bf935d1fa0")), // Ink
            81457 => Some(address!("eabbcb3e8e415306207ef514f660a3f820025be3")), // Blast
            // Testnets
            1301 => Some(address!("f70536b3bcc1bd1a972dc186a2cf84cc6da6be5d")), // Unichain Sepolia
            11155111 => Some(address!("3A9D48AB9751398BbFa63ad67599Bb04e4BdF98b")), // Sepolia
            84532 => Some(address!("492e6456d9528771018deb9e87ef7750ef184104")), // Base Sepolia
            421614 => Some(address!("efd1d4bd4cf1e86da286bb4cb1b8bced9c10ba47")), // Arbitrum Sepolia
            _ => None,
        }
    }

    /// Get the Quoter address for a given chain ID.
    #[must_use]
    pub const fn quoter_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(address!("52f0e24d1c21c8a0cb1e5a5dd6198556bd9e1203")), // Ethereum
            10 => Some(address!("1f3131a13296fb91c90870043742c3cdbff1a8d7")), // Optimism
            56 => Some(address!("9f75dd27d6664c475b90e105573e550ff69437b0")), // BNB
            130 => Some(address!("333e3c607b141b18ff6de9f258db6e77fe7491e0")), // Unichain
            137 => Some(address!("b3d5c3dfc3a7aebff71895a7191796bffc2c81b9")), // Polygon
            480 => Some(address!("55d235b3ff2daf7c3ede0defc9521f1d6fe6c5c0")), // Worldchain
            1868 => Some(address!("3972c00f7ed4885e145823eb7c655375d275a1c5")), // Soneium
            7777777 => Some(address!("5edaccc0660e0a2c44b06e07ce8b915e625dc2c6")), // Zora
            8453 => Some(address!("0d5e0f971ed27fbff6c2837bf31316121532048d")), // Base
            42161 => Some(address!("3972c00f7ed4885e145823eb7c655375d275a1c5")), // Arbitrum
            42220 => Some(address!("28566da1093609182dff2cb2a91cfd72e61d66cd")), // Celo
            43114 => Some(address!("be40675bb704506a3c2ccfb762dcfd1e979845c2")), // Avalanche
            57073 => Some(address!("3972c00f7ed4885e145823eb7c655375d275a1c5")), // Ink
            81457 => Some(address!("6f71cdcb0d119ff72c6eb501abceb576fbf62bcf")), // Blast
            // Testnets
            1301 => Some(address!("56dcd40a3f2d466f48e7f48bdbe5cc9b92ae4472")), // Unichain Sepolia
            11155111 => Some(address!("61b3f2011a92d183c7dbadbda940a7555ccf9227")), // Sepolia
            84532 => Some(address!("4a6513c898fe1b2d0e78d3b0e0a4a151589b1cba")), // Base Sepolia
            421614 => Some(address!("7de51022d70a725b508085468052e25e22b5c4c9")), // Arbitrum Sepolia
            _ => None,
        }
    }

    /// Get the PositionManager address for a given chain ID.
    #[must_use]
    pub const fn position_manager_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(address!("bd216513d74c8cf14cf4747e6aaa6420ff64ee9e")), // Ethereum
            10 => Some(address!("3c3ea4b57a46241e54610e5f022e5c45859a1017")), // Optimism
            56 => Some(address!("7a4a5c919ae2541aed11041a1aeee68f1287f95b")), // BNB
            130 => Some(address!("4529a01c7a0410167c5740c487a8de60232617bf")), // Unichain
            137 => Some(address!("1ec2ebf4f37e7363fdfe3551602425af0b3ceef9")), // Polygon
            480 => Some(address!("c585e0f504613b5fbf874f21af14c65260fb41fa")), // Worldchain
            1868 => Some(address!("1b35d13a2e2528f192637f14b05f0dc0e7deb566")), // Soneium
            7777777 => Some(address!("f66c7b99e2040f0d9b326b3b7c152e9663543d63")), // Zora
            8453 => Some(address!("7c5f5a4bbd8fd63184577525326123b519429bdc")), // Base
            42161 => Some(address!("d88f38f930b7952f2db2432cb002e7abbf3dd869")), // Arbitrum
            42220 => Some(address!("f7965f3981e4d5bc383bfbcb61501763e9068ca9")), // Celo
            43114 => Some(address!("b74b1f14d2754acfcbbe1a221023a5cf50ab8acd")), // Avalanche
            57073 => Some(address!("1b35d13a2e2528f192637f14b05f0dc0e7deb566")), // Ink
            81457 => Some(address!("4ad2f4cca2682cbb5b950d660dd458a1d3f1baad")), // Blast
            // Testnets
            1301 => Some(address!("f969aee60879c54baaed9f3ed26147db216fd664")), // Unichain Sepolia
            11155111 => Some(address!("429ba70129df741B2Ca2a85BC3A2a3328e5c09b4")), // Sepolia
            84532 => Some(address!("4b2c77d209d3405f41a037ec6c77f7f5b8e2ca80")), // Base Sepolia
            421614 => Some(address!("Ac631556d3d4019C95769033B5E719dD77124BAc")), // Arbitrum Sepolia
            _ => None,
        }
    }

    /// Get the PositionDescriptor address for a given chain ID.
    #[must_use]
    pub const fn position_descriptor_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(address!("d1428ba554f4c8450b763a0b2040a4935c63f06c")), // Ethereum
            10 => Some(address!("edd81496169c46df161b8513a52ffecaaaa66743")), // Optimism
            56 => Some(address!("f0432f360703ec3d33931a8356a75a77d8d380e1")), // BNB
            130 => Some(address!("9fb28449a191cd8c03a1b7abfb0f5996ecf7f722")), // Unichain
            137 => Some(address!("0892771f0c1b78ad6013d6e5536007e1c16e6794")), // Polygon
            480 => Some(address!("7da419153bd420b689f312363756d76836aeace4")), // Worldchain
            1868 => Some(address!("42e3ccd9b7f67b5b2ee0c12074b84ccf2a8e7f36")), // Soneium
            7777777 => Some(address!("7d64630bbb4993b5578dbd65e400961c9e68d55a")), // Zora
            8453 => Some(address!("25d093633990dc94bedeed76c8f3cdaa75f3e7d5")), // Base
            42161 => Some(address!("e2023f3fa515cf070e07fd9d51c1d236e07843f4")), // Arbitrum
            42220 => Some(address!("5727E22b25fEEe05E8dFa83C752B86F19D102D8A")), // Celo
            43114 => Some(address!("2b1aed9445b05ac1a3b203eccc1e25dd9351f0a9")), // Avalanche
            57073 => Some(address!("42e3ccd9b7f67b5b2ee0c12074b84ccf2a8e7f36")), // Ink
            81457 => Some(address!("0747ad2b2e1f5761b1dcf0d8672bd1ffc3676f97")), // Blast
            _ => None,
        }
    }

    /// Permit2 is deployed at the same address on all chains.
    pub const PERMIT2: Address = address!("000000000022D473030F116dDEE9F6B43aC78BA3");
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_compute_pool_id() {
        let currency0 = address!("0000000000000000000000000000000000000001");
        let currency1 = address!("0000000000000000000000000000000000000002");
        let fee = 3000u32;
        let tick_spacing = 60i32;
        let hooks = Address::ZERO;

        let pool_id = UniswapV4PoolManagerContract::compute_pool_id(
            currency0,
            currency1,
            fee,
            tick_spacing,
            hooks,
        );

        assert!(!pool_id.is_zero());
    }

    #[rstest]
    fn test_pool_manager_addresses() {
        use super::deployments::*;

        // Ethereum mainnet
        assert!(pool_manager_address(1).is_some());

        // Arbitrum
        assert!(pool_manager_address(42161).is_some());

        // Base
        assert!(pool_manager_address(8453).is_some());

        // Unknown chain
        assert!(pool_manager_address(999999).is_none());
    }
}

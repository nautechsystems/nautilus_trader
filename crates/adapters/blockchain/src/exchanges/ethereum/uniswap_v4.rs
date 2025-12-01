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

use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// Uniswap V4 DEX on Ethereum.
///
/// V4 uses a singleton PoolManager architecture instead of per-pool factory contracts.
/// The PoolManager address is used as the "factory" for event filtering purposes.
/// Pools are identified by PoolKey hash (pool_id) and created via Initialize events.
pub static UNISWAP_V4: LazyLock<DexExtended> = LazyLock::new(|| {
    // V4 uses PoolManager singleton instead of factory - use PoolManager address for event filtering
    // PoolManager deployed at block 21688329 on Ethereum mainnet
    let mut dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV4,
        "0x000000000004444c5dc75cB358380D2e3dE08A90", // PoolManager address (acts as factory)
        21688329,                                     // PoolManager deployment block
        AmmType::CLAMEnhanced,
        // V4 uses Initialize instead of PoolCreated - pools are created via initialize()
        "Initialize(bytes32,address,address,uint24,int24,address,uint160,int24)",
        // V4 Swap event includes fee parameter
        "Swap(bytes32,address,int128,int128,uint160,uint128,int24,uint24)",
        // V4 uses ModifyLiquidity instead of separate Mint/Burn - positive delta = add, negative = remove
        "ModifyLiquidity(bytes32,address,int24,int24,int256,bytes32)",
        // V4 ModifyLiquidity handles both add and remove liquidity
        "ModifyLiquidity(bytes32,address,int24,int24,int256,bytes32)",
        // V4 has Donate event instead of Collect for fee donations
        "Donate(bytes32,address,uint256,uint256)",
    );
    // V4 Initialize event serves as pool creation
    dex.set_initialize_event(
        "Initialize(bytes32,address,address,uint24,int24,address,uint160,int24)",
    );
    DexExtended::new(dex)
});

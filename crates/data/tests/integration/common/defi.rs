// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{str::FromStr, sync::Arc};

use alloy_primitives::{Address, I256, U160};
use nautilus_core::UnixNanos;
use nautilus_model::defi::{
    AmmType, Dex, DexType, Pool, PoolIdentifier, PoolSwap, Token, chain::chains,
    tick_map::tick_math::get_tick_at_sqrt_ratio,
};

pub(crate) fn make_initialized_pool_and_swap() -> (Pool, PoolSwap) {
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );
    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));

    let swap = PoolSwap::new(
        chain,
        dex,
        pool.instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
    );
    (pool, swap)
}

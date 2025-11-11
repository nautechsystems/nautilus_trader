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

use std::sync::Arc;

use alloy_primitives::address;
use nautilus_core::UnixNanos;
use rstest::fixture;

use crate::defi::{AmmType, Chain, Dex, DexType, Pool, SharedChain, SharedDex, SharedPool, Token};

#[fixture]
pub fn arbitrum() -> SharedChain {
    Arc::new(Chain::from_chain_id(42161).unwrap().clone())
}

#[fixture]
pub fn uniswap_v3() -> SharedDex {
    let arbitrum = arbitrum();
    let dex = Dex::new(
        (*arbitrum).clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    Arc::new(dex)
}

#[fixture]
pub fn weth(arbitrum: SharedChain) -> Token {
    Token::new(
        arbitrum,
        address!("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
        "Wrapped Ether".to_string(),
        "WETH".to_string(),
        18,
    )
}

#[fixture]
pub fn usdc(arbitrum: SharedChain) -> Token {
    Token::new(
        arbitrum,
        address!("0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8"),
        "USD Coin".to_string(),
        "USDC".to_string(),
        6, // USDC.e on Arbitrum has 6 decimals
    )
}

#[fixture]
pub fn rain_token(arbitrum: SharedChain) -> Token {
    Token::new(
        arbitrum,
        address!("0x25118290e6A5f4139381D072181157035864099d"),
        "RAIN".to_string(),
        "RAIN".to_string(),
        18,
    )
}

#[fixture]
pub fn rain_pool(
    arbitrum: SharedChain,
    uniswap_v3: SharedDex,
    rain_token: Token,
    weth: Token,
) -> SharedPool {
    let pool = Pool::new(
        arbitrum,
        uniswap_v3,
        address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245"),
        0,
        rain_token,
        weth,
        Some(100),
        Some(1),
        UnixNanos::default(),
    );

    Arc::new(pool)
}

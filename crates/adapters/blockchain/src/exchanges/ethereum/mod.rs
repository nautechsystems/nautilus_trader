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

use std::collections::HashMap;

use alloy::primitives::Address;

use crate::exchanges::extended::DexExtended;

pub mod balancer_v2;
pub mod balancer_v3;
pub mod curve_finance;
pub mod fluid;
pub mod maverick_v2;
pub mod pancakeswap_v3;
pub mod uniswap_v2;
pub mod uniswap_v3;
pub mod uniswap_v4;

pub use balancer_v2::BALANCER_V2;
pub use balancer_v3::BALANCER_V3;
pub use curve_finance::CURVE_FINANCE;
pub use fluid::FLUID_DEX;
pub use maverick_v2::MAVERICK_V2;
pub use pancakeswap_v3::PANCAKESWAP_V3;
pub use uniswap_v2::UNISWAP_V2;
pub use uniswap_v3::UNISWAP_V3;
pub use uniswap_v4::UNISWAP_V4;

/// Returns a slice of all Ethereum Dexes
#[must_use]
pub fn all() -> Vec<&'static DexExtended> {
    vec![
        &UNISWAP_V2,
        &UNISWAP_V3,
        &UNISWAP_V4,
        &CURVE_FINANCE,
        &FLUID_DEX,
        &MAVERICK_V2,
        &BALANCER_V2,
        &BALANCER_V3,
        &PANCAKESWAP_V3,
    ]
}

/// Returns a map of Ethereum DEX name to Dex reference for easy lookup
#[must_use]
pub fn dex_map() -> HashMap<&'static str, &'static DexExtended> {
    let mut map = HashMap::new();
    map.insert("uniswap_v2", &*UNISWAP_V2);
    map.insert("uniswap_v3", &*UNISWAP_V3);
    map.insert("uniswap_v4", &*UNISWAP_V4);
    map.insert("curve_finance", &*CURVE_FINANCE);
    map.insert("fluid_dex", &FLUID_DEX);
    map.insert("maverick_v2", &*MAVERICK_V2);
    map.insert("balancer_v2", &*BALANCER_V2);
    map.insert("balancer_v3", &*BALANCER_V3);
    map.insert("pancakeswap_v3", &*PANCAKESWAP_V3);
    map
}

/// Returns the token symbol for a given Ethereum token address.
/// Falls back to address-based naming for unknown tokens.
#[must_use]
pub fn get_token_symbol(token_address: Address) -> String {
    match token_address.to_string().to_lowercase().as_str() {
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => "WETH".to_string(),
        "0xa0b86a33e6441b936662bb6b5d1f8fb0e2b57a5d" => "USDC".to_string(),
        "0xdac17f958d2ee523a2206206994597c13d831ec7" => "USDT".to_string(),
        "0x6b175474e89094c44da98b954eedeac495271d0f" => "DAI".to_string(),
        "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599" => "WBTC".to_string(),
        "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984" => "UNI".to_string(),
        "0x514910771af9ca656af840dff83e8264ecf986ca" => "LINK".to_string(),
        "0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9" => "AAVE".to_string(),
        "0x4fabb145d64652a948d72533023f6e7a623c7c53" => "BUSD".to_string(),
        _ => format!(
            "TOKEN_{addr}",
            addr = &token_address.to_string()[2..8].to_uppercase()
        ),
    }
}

/// Returns the token address for a given Ethereum token symbol.
#[must_use]
pub fn get_token_symbol_reverse(symbol: &str) -> anyhow::Result<Address> {
    match symbol {
        "WETH" => Ok("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2".parse()?),
        "USDC" => Ok("0xa0b86a33e6441b936662bb6b5d1f8fb0e2b57a5d".parse()?),
        "USDT" => Ok("0xdac17f958d2ee523a2206206994597c13d831ec7".parse()?),
        "DAI" => Ok("0x6b175474e89094c44da98b954eedeac495271d0f".parse()?),
        "WBTC" => Ok("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599".parse()?),
        "UNI" => Ok("0x1f9840a85d5af5bf1d1762f925bdaddc4201f984".parse()?),
        "LINK" => Ok("0x514910771af9ca656af840dff83e8264ecf986ca".parse()?),
        "AAVE" => Ok("0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9".parse()?),
        "BUSD" => Ok("0x4fabb145d64652a948d72533023f6e7a623c7c53".parse()?),
        _ => anyhow::bail!("Unknown token symbol for Ethereum: {symbol}"),
    }
}

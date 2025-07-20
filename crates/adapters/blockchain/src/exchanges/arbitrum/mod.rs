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

mod camelot_v3;
mod curve_finance;
mod fluid;
mod pancakeswap_v3;
mod sushiswap_v2;
mod sushiswap_v3;
mod uniswap_v3;
mod uniswap_v4;

pub use camelot_v3::CAMELOT_V3;
pub use curve_finance::CURVE_FINANCE;
pub use fluid::FLUID_DEX;
pub use pancakeswap_v3::PANCAKESWAP_V3;
pub use sushiswap_v2::SUSHISWAP_V2;
pub use sushiswap_v3::SUSHISWAP_V3;
pub use uniswap_v3::UNISWAP_V3;
pub use uniswap_v4::UNISWAP_V4;

/// Returns a vector of references to all Arbitrum Dexes.
#[must_use]
pub fn all() -> Vec<&'static DexExtended> {
    vec![
        &*CAMELOT_V3,
        &*CURVE_FINANCE,
        &*FLUID_DEX,
        &*PANCAKESWAP_V3,
        &*SUSHISWAP_V2,
        &*SUSHISWAP_V3,
        &*UNISWAP_V3,
        &*UNISWAP_V4,
    ]
}

/// Returns a map of Arbitrum DEX name to Dex reference for easy lookup.
#[must_use]
pub fn dex_map() -> HashMap<String, &'static DexExtended> {
    let mut map = HashMap::new();
    map.insert(CAMELOT_V3.id(), &*CAMELOT_V3);
    map.insert(CURVE_FINANCE.id(), &*CURVE_FINANCE);
    map.insert(FLUID_DEX.id(), &*FLUID_DEX);
    map.insert(PANCAKESWAP_V3.id(), &*PANCAKESWAP_V3);
    map.insert(SUSHISWAP_V2.id(), &*SUSHISWAP_V2);
    map.insert(SUSHISWAP_V3.id(), &*SUSHISWAP_V3);
    map.insert(UNISWAP_V3.id(), &*UNISWAP_V3);
    map.insert(UNISWAP_V4.id(), &*UNISWAP_V4);
    map
}

/// Returns the token symbol for a given Arbitrum token address.
/// Falls back to address-based naming for unknown tokens.
#[must_use]
pub fn get_token_symbol(token_address: Address) -> String {
    match token_address.to_string().to_lowercase().as_str() {
        "0x82af49447d8a07e3bd95bd0d56f35241523fbab1" => "WETH".to_string(),
        "0xaf88d065e77c8cc2239327c5edb3a432268e5831" => "USDC".to_string(),
        "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9" => "USDT".to_string(),
        "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1" => "DAI".to_string(),
        "0x2f2a2543b76a4166549f7aab2e75bef0aefc5b0f" => "WBTC".to_string(),
        "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8" => "USDC.e".to_string(),
        "0x912ce59144191c1204e64559fe8253a0e49e6548" => "ARB".to_string(),
        "0xf97f4df75117a78c1a5a0dbb814af92458539fb4" => "LINK".to_string(),
        "0xfa7f8980b0f1e64a2062791cc3b0871572f1f7f0" => "UNI".to_string(),
        _ => format!(
            "TOKEN_{addr}",
            addr = &token_address.to_string()[2..8].to_uppercase()
        ),
    }
}

/// Returns the token address for a given Arbitrum token symbol.
#[must_use]
pub fn get_token_symbol_reverse(symbol: &str) -> anyhow::Result<Address> {
    match symbol {
        "WETH" => Ok("0x82af49447d8a07e3bd95bd0d56f35241523fbab1".parse()?),
        "USDC" => Ok("0xaf88d065e77c8cc2239327c5edb3a432268e5831".parse()?),
        "USDT" => Ok("0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9".parse()?),
        "DAI" => Ok("0xda10009cbd5d07dd0cecc66161fc93d7c9000da1".parse()?),
        "WBTC" => Ok("0x2f2a2543b76a4166549f7aab2e75bef0aefc5b0f".parse()?),
        "USDC.e" => Ok("0xff970a61a04b1ca14834a43f5de4533ebddb5cc8".parse()?),
        "ARB" => Ok("0x912ce59144191c1204e64559fe8253a0e49e6548".parse()?),
        "LINK" => Ok("0xf97f4df75117a78c1a5a0dbb814af92458539fb4".parse()?),
        "UNI" => Ok("0xfa7f8980b0f1e64a2062791cc3b0871572f1f7f0".parse()?),
        _ => anyhow::bail!("Unknown token symbol for Arbitrum: {symbol}"),
    }
}

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

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v3};

/// Uniswap V3 DEX on Ethereum.
pub static UNISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        12369621,
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
        "Mint(address,address,int24,int24,uint128,uint256,uint256)",
        "Burn(address,int24,int24,uint128,uint256,uint256)",
        "Collect(address,address,int24,int24,uint128,uint128)",
    );
    dex.set_initialize_event("Initialize(uint160,int24)");
    dex.set_flash_event("Flash(address,address,uint256,uint256,uint256,uint256)");
    let mut dex_extended = DexExtended::new(dex);

    dex_extended.set_pool_created_event_parsing(uniswap_v3::pool_created::parse_pool_created_event);
    dex_extended.set_swap_event_parsing(uniswap_v3::swap::parse_swap_event);
    dex_extended.set_convert_trade_data(uniswap_v3::trade_data::convert_to_trade_data);
    dex_extended.set_mint_event_parsing(uniswap_v3::mint::parse_mint_event);
    dex_extended.set_burn_event_parsing(uniswap_v3::burn::parse_burn_event);
    dex_extended.set_collect_event_parsing(uniswap_v3::collect::parse_collect_event);
    dex_extended.set_flash_event_parsing(uniswap_v3::flash::parse_flash_event);

    dex_extended
});

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

use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v3};

/// Aerodrome Slipstream DEX on Base.
///
/// Slipstream is a Uniswap V3 fork; pool events reuse the V3 parsers. The factory
/// PoolCreated layout differs (tickSpacing in place of fee) and is left empty until
/// a Slipstream parser is written, since advertising a topic without a parser would
/// abort pool discovery.
pub static AERODROME_SLIPSTREAM: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = Dex::new(
        chains::BASE.clone(),
        DexType::AerodromeSlipstream,
        "0x420DD381b31aEf6683db6B902084cB0FFECe40Da",
        3200559,
        AmmType::CLAMM,
        "",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
        "Mint(address,address,int24,int24,uint128,uint256,uint256)",
        "Burn(address,int24,int24,uint128,uint256,uint256)",
        "Collect(address,address,int24,int24,uint128,uint128)",
    );
    dex.set_initialize_event("Initialize(uint160,int24)");
    dex.set_flash_event("Flash(address,address,uint256,uint256,uint256,uint256)");

    let mut dex_extended = DexExtended::new(dex);

    dex_extended.set_initialize_event_hypersync_parsing(
        uniswap_v3::initialize::parse_initialize_event_hypersync,
    );
    dex_extended.set_swap_event_hypersync_parsing(uniswap_v3::swap::parse_swap_event_hypersync);
    dex_extended.set_mint_event_hypersync_parsing(uniswap_v3::mint::parse_mint_event_hypersync);
    dex_extended.set_burn_event_hypersync_parsing(uniswap_v3::burn::parse_burn_event_hypersync);
    dex_extended
        .set_collect_event_hypersync_parsing(uniswap_v3::collect::parse_collect_event_hypersync);
    dex_extended.set_flash_event_hypersync_parsing(uniswap_v3::flash::parse_flash_event_hypersync);

    dex_extended
        .set_initialize_event_rpc_parsing(uniswap_v3::initialize::parse_initialize_event_rpc);
    dex_extended.set_swap_event_rpc_parsing(uniswap_v3::swap::parse_swap_event_rpc);
    dex_extended.set_mint_event_rpc_parsing(uniswap_v3::mint::parse_mint_event_rpc);
    dex_extended.set_burn_event_rpc_parsing(uniswap_v3::burn::parse_burn_event_rpc);
    dex_extended.set_collect_event_rpc_parsing(uniswap_v3::collect::parse_collect_event_rpc);
    dex_extended.set_flash_event_rpc_parsing(uniswap_v3::flash::parse_flash_event_rpc);

    dex_extended
});

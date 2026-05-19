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

/// PancakeSwap V3 DEX on BSC.
/// Factory: <https://bscscan.com/address/0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865>
pub static PANCAKESWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    // PoolCreated matches the V3 layout; pool-event shapes differ (Swap adds
    // protocolFees fields) so those filters stay empty until parsers are written.
    let dex = Dex::new(
        chains::BSC.clone(),
        DexType::PancakeSwapV3,
        "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
        26956207,
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "",
        "",
        "",
        "",
    );
    let mut dex_extended = DexExtended::new(dex);

    dex_extended.set_pool_created_event_hypersync_parsing(
        uniswap_v3::pool_created::parse_pool_created_event_hypersync,
    );
    dex_extended
        .set_pool_created_event_rpc_parsing(uniswap_v3::pool_created::parse_pool_created_event_rpc);

    dex_extended
});

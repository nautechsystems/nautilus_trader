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

/// SushiSwap V3 DEX on Arbitrum.
pub static SUSHISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ARBITRUM.clone(),
        DexType::SushiSwapV3,
        "0x1af415a1EbA07a4986a52B6f2e7dE7003D82231e",
        76082155,
        AmmType::CLAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

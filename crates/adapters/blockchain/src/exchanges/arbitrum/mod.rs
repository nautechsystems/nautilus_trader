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
pub fn dex_map() -> HashMap<&'static str, &'static DexExtended> {
    let mut map = HashMap::new();
    map.insert("camelot_v3", &*CAMELOT_V3);
    map.insert("curve_finance", &*CURVE_FINANCE);
    map.insert("fluid_dex", &*FLUID_DEX);
    map.insert("pancakeswap_v3", &*PANCAKESWAP_V3);
    map.insert("sushiswap_v2", &*SUSHISWAP_V2);
    map.insert("sushiswap_v3", &*SUSHISWAP_V3);
    map.insert("uniswap_v3", &*UNISWAP_V3);
    map.insert("uniswap_v4", &*UNISWAP_V4);
    map
}

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

use std::{collections::HashMap, sync::LazyLock};

use crate::exchanges::extended::DexExtended;

mod aerodrome_slipstream;
mod aerodrome_v1;
mod baseswap_v2;
mod basex;
mod maverick_v1;
mod maverick_v2;
mod pancakeswap_v3;
mod sushiswap_v3;
mod uniswap_v2;
mod uniswap_v3;
mod uniswap_v4;

pub use aerodrome_slipstream::AERODROME_SLIPSTREAM;
pub use aerodrome_v1::AERODROME_V1;
pub use baseswap_v2::BASESWAP_V2;
pub use basex::BASEX;
pub use maverick_v1::MAVERICK_V1;
pub use maverick_v2::MAVERICK_V2;
use nautilus_model::defi::DexType;
pub use pancakeswap_v3::PANCAKESWAP_V3;
pub use sushiswap_v3::SUSHISWAP_V3;
pub use uniswap_v2::UNISWAP_V2;
pub use uniswap_v3::UNISWAP_V3;
pub use uniswap_v4::UNISWAP_V4;

pub static BASE_DEX_EXTENDED_MAP: LazyLock<HashMap<DexType, &'static DexExtended>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        map.insert(AERODROME_SLIPSTREAM.dex.name, &*AERODROME_SLIPSTREAM);
        map.insert(AERODROME_V1.dex.name, &*AERODROME_V1);
        map.insert(UNISWAP_V2.dex.name, &*UNISWAP_V2);
        map.insert(UNISWAP_V3.dex.name, &*UNISWAP_V3);
        map.insert(UNISWAP_V4.dex.name, &*UNISWAP_V4);
        map.insert(PANCAKESWAP_V3.dex.name, &*PANCAKESWAP_V3);
        map.insert(MAVERICK_V1.dex.name, &*MAVERICK_V1);
        map.insert(MAVERICK_V2.dex.name, &*MAVERICK_V2);
        map.insert(SUSHISWAP_V3.dex.name, &*SUSHISWAP_V3);
        map.insert(BASEX.dex.name, &*BASEX);
        map.insert(BASESWAP_V2.dex.name, &*BASESWAP_V2);
        map
    });

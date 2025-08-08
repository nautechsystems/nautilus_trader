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
use nautilus_model::defi::DexType;
pub use pancakeswap_v3::PANCAKESWAP_V3;
pub use uniswap_v2::UNISWAP_V2;
pub use uniswap_v3::UNISWAP_V3;
pub use uniswap_v4::UNISWAP_V4;

pub static ETHEREUM_DEX_EXTENDED_MAP: LazyLock<HashMap<DexType, &'static DexExtended>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        map.insert(UNISWAP_V2.dex.name, &*UNISWAP_V2);
        map.insert(UNISWAP_V3.dex.name, &*UNISWAP_V3);
        map.insert(UNISWAP_V4.dex.name, &*UNISWAP_V4);
        map.insert(CURVE_FINANCE.dex.name, &*CURVE_FINANCE);
        map.insert(FLUID_DEX.dex.name, &*FLUID_DEX);
        map.insert(MAVERICK_V2.dex.name, &*MAVERICK_V2);
        map.insert(BALANCER_V2.dex.name, &*BALANCER_V2);
        map.insert(BALANCER_V3.dex.name, &*BALANCER_V3);
        map.insert(PANCAKESWAP_V3.dex.name, &*PANCAKESWAP_V3);

        map
    });

use std::{collections::HashMap, sync::LazyLock};

use nautilus_model::defi::DexType;

use crate::exchanges::extended::DexExtended;

mod aerodrome_slipstream;
mod aerodrome_v1;
mod baseswap_v2;
mod basex;
mod pancakeswap_v3;
mod sushiswap_v3;
mod uniswap_v2;
mod uniswap_v3;
mod uniswap_v4;

pub use aerodrome_slipstream::AERODROME_SLIPSTREAM;
pub use aerodrome_v1::AERODROME_V1;
pub use baseswap_v2::BASESWAP_V2;
pub use basex::BASEX;
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
        map.insert(SUSHISWAP_V3.dex.name, &*SUSHISWAP_V3);
        map.insert(BASEX.dex.name, &*BASEX);
        map.insert(BASESWAP_V2.dex.name, &*BASESWAP_V2);
        map
    });

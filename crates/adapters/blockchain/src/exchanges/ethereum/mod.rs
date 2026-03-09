use std::{collections::HashMap, sync::LazyLock};

use nautilus_model::defi::DexType;

use crate::exchanges::extended::DexExtended;

pub mod curve_finance;
pub mod fluid;
pub mod pancakeswap_v3;
pub mod uniswap_v2;
pub mod uniswap_v3;
pub mod uniswap_v4;

pub use curve_finance::CURVE_FINANCE;
pub use fluid::FLUID_DEX;
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
        map.insert(PANCAKESWAP_V3.dex.name, &*PANCAKESWAP_V3);

        map
    });

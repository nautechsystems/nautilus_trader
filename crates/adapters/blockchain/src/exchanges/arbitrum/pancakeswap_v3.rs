use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v3};

/// PancakeSwap V3 DEX on Arbitrum.
pub static PANCAKESWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = DexExtended::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::PancakeSwapV3,
        "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
        105068129,
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "",
        "",
        "",
        "",
    ));
    dex.set_pool_created_event_hypersync_parsing(
        uniswap_v3::pool_created::parse_pool_created_event_hypersync,
    );
    dex
});

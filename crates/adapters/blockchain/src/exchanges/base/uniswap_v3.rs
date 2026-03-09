use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v3};

/// Uniswap V3 DEX on Base.
pub static UNISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::BASE.clone(),
        DexType::UniswapV3,
        "0x33128a8fC17869897dcE68Ed026d694621f6FDfD",
        1371680,
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
        "Mint(address,address,int24,int24,uint128,uint256,uint256)",
        "Burn(address,int24,int24,uint128,uint256,uint256)",
        "Collect(address,int24,int24,uint128,uint128)",
    );

    let mut dex_extended = DexExtended::new(dex);

    // HyperSync parsers
    dex_extended.set_pool_created_event_hypersync_parsing(
        uniswap_v3::pool_created::parse_pool_created_event_hypersync,
    );
    dex_extended.set_initialize_event_hypersync_parsing(
        uniswap_v3::initialize::parse_initialize_event_hypersync,
    );

    dex_extended
});

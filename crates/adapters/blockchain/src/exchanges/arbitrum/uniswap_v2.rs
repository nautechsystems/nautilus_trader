use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v2};

/// Uniswap V2 DEX on Arbitrum.
/// Factory: <https://arbiscan.io/address/0xf1D7CC64Fb4452F05c498126312eBE29f30Fbcf9>
pub static UNISWAP_V2: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV2,
        "0xf1D7CC64Fb4452F05c498126312eBE29f30Fbcf9",
        150442611,
        AmmType::CPAMM,
        "PairCreated(address,address,address,uint256)",
        "",
        "",
        "",
        "",
    );

    let mut dex_extended = DexExtended::new(dex);

    // Register PairCreated event parsers
    dex_extended.set_pool_created_event_hypersync_parsing(
        uniswap_v2::pool_created::parse_pool_created_event_hypersync,
    );
    dex_extended
        .set_pool_created_event_rpc_parsing(uniswap_v2::pool_created::parse_pool_created_event_rpc);

    dex_extended
});

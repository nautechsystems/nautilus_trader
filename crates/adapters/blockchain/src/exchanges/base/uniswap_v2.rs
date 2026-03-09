use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v2};

/// Uniswap V2 DEX on Base.
pub static UNISWAP_V2: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::BASE.clone(),
        DexType::UniswapV2,
        "0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6",
        6601915,
        AmmType::CPAMM,
        "PairCreated(address,address,address,uint256)",
        "",
        "",
        "",
        "",
    );
    let mut dex_extended = DexExtended::new(dex);

    dex_extended.set_pool_created_event_hypersync_parsing(
        uniswap_v2::pool_created::parse_pool_created_event_hypersync,
    );
    dex_extended
        .set_pool_created_event_rpc_parsing(uniswap_v2::pool_created::parse_pool_created_event_rpc);

    dex_extended
});

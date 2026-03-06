use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::{extended::DexExtended, parsing::uniswap_v4};

/// Uniswap V4 DEX on Ethereum.
pub static UNISWAP_V4: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV4,
        "0x000000000004444c5dc75cB358380D2e3dE08A90",
        21688329,
        AmmType::CLAMEnhanced,
        "Initialize(bytes32,address,address,uint24,int24,address,uint160,int24)",
        "",
        "",
        "",
        "",
    );

    let mut dex_extended = DexExtended::new(dex);

    // Register Initialize event as pool discovery mechanism
    dex_extended.set_pool_created_event_hypersync_parsing(
        uniswap_v4::initialize::parse_initialize_event_hypersync,
    );
    dex_extended
        .set_pool_created_event_rpc_parsing(uniswap_v4::initialize::parse_initialize_event_rpc);

    dex_extended
});

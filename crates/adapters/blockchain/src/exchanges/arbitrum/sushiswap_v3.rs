use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// SushiSwap V3 DEX on Arbitrum.
pub static SUSHISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ARBITRUM.clone(),
        DexType::SushiSwapV3,
        "0x1af415a1EbA07a4986a52B6f2e7dE7003D82231e",
        76082155,
        AmmType::CLAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

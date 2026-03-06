use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// SushiSwap V2 DEX on Arbitrum.
pub static SUSHISWAP_V2: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ARBITRUM.clone(),
        DexType::SushiSwapV2,
        "0xc35DADB65012eC5796536bD9864eD8773aBc74C4",
        70,
        AmmType::CPAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

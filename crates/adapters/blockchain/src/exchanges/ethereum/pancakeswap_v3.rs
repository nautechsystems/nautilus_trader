use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// PancakeSwap V3 DEX on Ethereum.
pub static PANCAKESWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::PancakeSwapV3,
        "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
        16950707,
        AmmType::CLAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

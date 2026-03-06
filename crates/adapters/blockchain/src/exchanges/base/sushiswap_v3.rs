use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// SushiSwap V3 DEX on Base.
pub static SUSHISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::BASE.clone(),
        DexType::SushiSwapV3,
        "0x93395129bd3fcf49d95730D3C2737c17990fF328",
        0,
        AmmType::CLAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

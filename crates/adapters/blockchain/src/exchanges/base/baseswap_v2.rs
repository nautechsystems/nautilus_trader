use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// BaseSwap V2 DEX on Base.
pub static BASESWAP_V2: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::BASE.clone(),
        DexType::BaseSwapV2,
        "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB",
        2059124,
        AmmType::CPAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

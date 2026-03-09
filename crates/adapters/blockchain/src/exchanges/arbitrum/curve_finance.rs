use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// Curve Finance DEX on Arbitrum.
pub static CURVE_FINANCE: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ARBITRUM.clone(),
        DexType::CurveFinance,
        "0xb17b674D9c5CB2e441F8e196a2f048A81355d031",
        1413161,
        AmmType::StableSwap,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

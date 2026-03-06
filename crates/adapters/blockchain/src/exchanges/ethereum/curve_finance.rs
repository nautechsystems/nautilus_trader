use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// Curve Finance DEX on Ethereum.
pub static CURVE_FINANCE: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::CurveFinance,
        "0xB9fC157394Af804a3578134A6585C0dc9cc990d4",
        12903979,
        AmmType::StableSwap,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

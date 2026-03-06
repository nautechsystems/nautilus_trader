use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// Aerodrome Slipstream DEX on Base.
pub static AERODROME_SLIPSTREAM: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::BASE.clone(),
        DexType::AerodromeSlipstream,
        "0x420DD381b31aEf6683db6B902084cB0FFECe40Da",
        3200559,
        AmmType::StableSwap,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

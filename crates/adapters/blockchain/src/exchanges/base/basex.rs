use std::sync::LazyLock;

use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::exchanges::extended::DexExtended;

/// BaseX DEX on Base.
pub static BASEX: LazyLock<DexExtended> = LazyLock::new(|| {
    let dex = Dex::new(
        chains::BASE.clone(),
        DexType::BaseX,
        "0x38015D05f4fEC8AFe15D7cc0386a126574e8077B",
        3608198,
        AmmType::CLAMM,
        "",
        "",
        "",
        "",
        "",
    );
    DexExtended::new(dex)
});

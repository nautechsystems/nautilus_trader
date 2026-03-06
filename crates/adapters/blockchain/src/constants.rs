use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;

pub const BLOCKCHAIN: &str = "BLOCKCHAIN";
pub static BLOCKCHAIN_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(BLOCKCHAIN));

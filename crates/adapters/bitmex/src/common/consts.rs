//! BitMEX adapter constants including base URLs and the venue identifier.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const BITMEX: &str = "BITMEX";

pub const BITMEX_WS_URL: &str = "wss://ws.bitmex.com/realtime";
pub const BITMEX_WS_TESTNET_URL: &str = "wss://ws.testnet.bitmex.com/realtime";
pub const BITMEX_HTTP_URL: &str = "https://www.bitmex.com/api/v1";
pub const BITMEX_HTTP_TESTNET_URL: &str = "https://testnet.bitmex.com/api/v1";

pub const BITMEX_WS_TOPIC_DELIMITER: char = ':';

pub static BITMEX_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BITMEX)));

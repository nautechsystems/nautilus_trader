//! Core constants shared across the Bybit adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const BYBIT: &str = "BYBIT";
pub static BYBIT_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BYBIT)));

pub const BYBIT_PONG: &str = "pong";

pub const BYBIT_BASE_COIN: &str = "baseCoin";
pub const BYBIT_QUOTE_COIN: &str = "quoteCoin";

/// See <https://www.bybit.com/en/broker> for further details.
pub const BYBIT_NAUTILUS_BROKER_ID: &str = "Qy000878";

pub const BYBIT_HTTP_URL: &str = "https://api.bybit.com";
pub const BYBIT_HTTP_TESTNET_URL: &str = "https://api-testnet.bybit.com";

pub const BYBIT_WS_PUBLIC_URL: &str = "wss://stream.bybit.com/v5/public/linear";
pub const BYBIT_WS_PRIVATE_URL: &str = "wss://stream.bybit.com/v5/private";

pub const BYBIT_WS_TESTNET_PUBLIC_URL: &str = "wss://stream-testnet.bybit.com/v5/public/linear";
pub const BYBIT_WS_TESTNET_PRIVATE_URL: &str = "wss://stream-testnet.bybit.com/v5/private";

pub const BYBIT_WS_TOPIC_DELIMITER: char = '.';

pub const BYBIT_TOPIC_ORDERBOOK: &str = "orderbook";
pub const BYBIT_TOPIC_TRADE: &str = "trade";
pub const BYBIT_TOPIC_PUBLIC_TRADE: &str = "publicTrade";
pub const BYBIT_TOPIC_KLINE: &str = "kline";
pub const BYBIT_TOPIC_TICKERS: &str = "tickers";
pub const BYBIT_TOPIC_ORDER: &str = "order";
pub const BYBIT_TOPIC_EXECUTION: &str = "execution";
pub const BYBIT_TOPIC_WALLET: &str = "wallet";
pub const BYBIT_TOPIC_POSITION: &str = "position";

pub const BYBIT_DEFAULT_ORDERBOOK_DEPTH: u32 = 50;

//! Binance Futures HTTP client with JSON encoding.

pub mod client;
pub mod error;
pub mod models;
pub mod query;

pub use client::{BinanceFuturesHttpClient, BinanceFuturesInstrument, BinanceRawFuturesHttpClient};
pub use error::{BinanceFuturesHttpError, BinanceFuturesHttpResult};
pub use models::{
    BinanceBookTicker, BinanceFundingRate, BinanceFuturesAsset, BinanceFuturesCoinExchangeInfo,
    BinanceFuturesCoinSymbol, BinanceFuturesMarkPrice, BinanceFuturesOrder,
    BinanceFuturesTicker24hr, BinanceFuturesUsdExchangeInfo, BinanceFuturesUsdSymbol,
    BinanceOpenInterest, BinanceOrderBook, BinancePriceTicker, BinanceServerTime,
    ListenKeyResponse,
};
pub use query::{
    BinanceBookTickerParams, BinanceDepthParams, BinanceFundingRateParams,
    BinanceIncomeHistoryParams, BinanceMarkPriceParams, BinanceOpenInterestParams,
    BinanceOpenOrdersParams, BinanceOrderQueryParams, BinancePositionRiskParams,
    BinanceTicker24hrParams, BinanceUserTradesParams, ListenKeyParams,
};

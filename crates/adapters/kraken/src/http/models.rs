//! Re-exports of HTTP API models for backwards compatibility.
//!
//! Models are now organized by product type:
//! - Spot models: [`super::spot::models`]
//! - Futures models: [`super::futures::models`]

// Re-exports
pub use super::{
    futures::models::{
        CancelledOrder, FuturesBatchOrderResponse, FuturesCancelAllOrdersResponse,
        FuturesCancelAllStatus, FuturesCancelOrderResponse, FuturesCancelStatus, FuturesCandle,
        FuturesCandlesResponse, FuturesEditOrderResponse, FuturesEditStatus, FuturesFill,
        FuturesFillsResponse, FuturesInstrument, FuturesInstrumentsResponse, FuturesMarginLevel,
        FuturesOpenOrder, FuturesOpenOrdersResponse, FuturesOpenPositionsResponse,
        FuturesOrderEvent, FuturesOrderEventsResponse, FuturesPosition, FuturesPublicExecution,
        FuturesPublicExecutionElement, FuturesPublicExecutionEvent, FuturesPublicExecutionWrapper,
        FuturesPublicExecutionsResponse, FuturesPublicOrder, FuturesSendOrderResponse,
        FuturesSendStatus, FuturesTicker, FuturesTickersResponse,
    },
    spot::models::{
        AssetPairInfo, AssetPairsResponse, KrakenResponse, OhlcData, OhlcResponse, OrderBookData,
        OrderBookLevel, OrderBookResponse, OrderDescription, ServerTime, SpotAddOrderResponse,
        SpotCancelOrderResponse, SpotClosedOrdersResult, SpotEditOrderResponse,
        SpotOpenOrdersResult, SpotOrder, SpotTrade, SpotTradesHistoryResult, SystemStatus,
        TickerInfo, TickerResponse, TradeData, TradesResponse, WebSocketToken,
    },
};

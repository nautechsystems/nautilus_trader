// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Response models for the Kalshi Trade API.
//!
//! Prices arrive as fixed-point dollar strings (`*_dollars`, up to four decimal places) and
//! contract counts as fixed-point strings (`*_fp`, two decimal places). They stay as strings here
//! and are parsed into exact values by [`crate::http::parse`]. Unknown response fields are ignored:
//! the exchange adds fields without notice.

use serde::{Deserialize, Serialize};

use crate::{
    common::enums::{
        KalshiBookSide, KalshiMarketResult, KalshiMarketStatus, KalshiMarketType, KalshiOrderSide,
        KalshiOrderType, KalshiOutcomeSide, KalshiSelfTradePrevention, KalshiTimeInForce,
    },
    http::error::KalshiApiErrorBody,
};

/// A band of valid prices, in fixed-point dollars.
///
/// `price_ranges` is the authority on a market's price grid: any price on the grid is accepted and
/// any price off it is rejected.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiPriceRange {
    /// The band's inclusive start price.
    pub start: String,
    /// The band's inclusive end price.
    pub end: String,
    /// The tick size inside the band.
    pub step: String,
}

/// One Kalshi market, the tradable unit of the exchange.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarket {
    /// The market ticker, unique across the exchange.
    pub ticker: String,
    /// The event the market belongs to.
    pub event_ticker: String,
    /// Whether the market is binary or settles on a scalar range.
    pub market_type: KalshiMarketType,
    /// The label of the YES side of the market.
    pub yes_sub_title: String,
    /// The label of the NO side of the market.
    pub no_sub_title: String,
    /// Time the market was created.
    pub created_time: String,
    /// Time of the last non-trading metadata update.
    pub updated_time: String,
    /// Time the market opened.
    pub open_time: String,
    /// Time the market closes for trading.
    pub close_time: String,
    /// The latest possible expiration time.
    pub latest_expiration_time: String,
    /// Seconds between determination and settlement.
    pub settlement_timer_seconds: i64,
    /// The market's position in its lifecycle.
    pub status: KalshiMarketStatus,
    /// The value of one contract at settlement, in fixed-point dollars.
    pub notional_value_dollars: String,
    /// The highest YES bid, in fixed-point dollars.
    pub yes_bid_dollars: String,
    /// The lowest YES ask, in fixed-point dollars.
    pub yes_ask_dollars: String,
    /// The highest NO bid, in fixed-point dollars.
    pub no_bid_dollars: String,
    /// The lowest NO ask, in fixed-point dollars.
    pub no_ask_dollars: String,
    /// Contracts bid at the best YES bid.
    pub yes_bid_size_fp: String,
    /// Contracts offered at the best YES ask.
    pub yes_ask_size_fp: String,
    /// The last traded YES price, in fixed-point dollars.
    pub last_price_dollars: String,
    /// The YES bid a day ago, in fixed-point dollars.
    pub previous_yes_bid_dollars: String,
    /// The YES ask a day ago, in fixed-point dollars.
    pub previous_yes_ask_dollars: String,
    /// The last traded YES price a day ago, in fixed-point dollars.
    pub previous_price_dollars: String,
    /// Total traded volume in contracts.
    pub volume_fp: String,
    /// Volume over the last 24 hours in contracts.
    pub volume_24h_fp: String,
    /// Open interest in contracts.
    pub open_interest_fp: String,
    /// The determined outcome, empty while the market is undetermined.
    pub result: KalshiMarketResult,
    /// Whether the market can close before its expiration.
    pub can_close_early: bool,
    /// The value considered for settlement.
    pub expiration_value: String,
    /// The primary settlement rules.
    pub rules_primary: String,
    /// The secondary settlement rules.
    pub rules_secondary: String,
    /// The name of the market's price grid.
    pub price_level_structure: String,
    /// The bands of valid prices.
    pub price_ranges: Vec<KalshiPriceRange>,
    /// The settlement value of the YES side, filled once determined.
    #[serde(default)]
    pub settlement_value_dollars: Option<String>,
    /// The time the market settled, filled once settled.
    #[serde(default)]
    pub settlement_ts: Option<String>,
    /// The time the market is expected to expire.
    #[serde(default)]
    pub expected_expiration_time: Option<String>,
    /// The time the underlying occurrence is expected.
    #[serde(default)]
    pub occurrence_datetime: Option<String>,
    /// The deprecated title of the market.
    #[serde(default)]
    pub title: Option<String>,
    /// The deprecated subtitle of the market.
    #[serde(default)]
    pub subtitle: Option<String>,
}

/// A response carrying many markets and a pagination cursor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarketsResponse {
    /// The markets on this page.
    pub markets: Vec<KalshiMarket>,
    /// The cursor for the next page, empty on the last page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// A response carrying one market.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarketResponse {
    /// The requested market.
    pub market: KalshiMarket,
}

/// One Kalshi event, the real-world occurrence markets hang off.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiEvent {
    /// The event ticker, unique across the exchange.
    pub event_ticker: String,
    /// The series the event belongs to.
    pub series_ticker: String,
    /// A shortened description of the event.
    pub sub_title: String,
    /// The full title of the event.
    pub title: String,
    /// How collateral is returned at settlement, such as `binary`.
    pub collateral_return_type: String,
    /// Whether only one market in the event can resolve to `yes`.
    pub mutually_exclusive: bool,
    /// The markets of the event, populated when the request nests them.
    #[serde(default)]
    pub markets: Vec<KalshiMarket>,
    /// The time the event's metadata last changed.
    #[serde(default)]
    pub last_updated_ts: Option<String>,
    /// The exchange shard serving the event.
    #[serde(default)]
    pub exchange_index: Option<i64>,
}

/// A response carrying many events and a pagination cursor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiEventsResponse {
    /// The events on this page.
    pub events: Vec<KalshiEvent>,
    /// The cursor for the next page, empty on the last page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// A response carrying one event and its markets.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiEventResponse {
    /// The requested event.
    pub event: KalshiEvent,
    /// The markets of the event.
    #[serde(default)]
    pub markets: Vec<KalshiMarket>,
}

/// One price level of the book: `[dollars, contracts]`.
///
/// The second element is a contract count, not a price.
pub type KalshiPriceLevel = (String, String);

/// The order book of a binary market.
///
/// Only bids are published: a YES bid at a price is a NO ask at one minus that price, so the two
/// sides of the book carry the same information.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderbook {
    /// YES bid levels, best first.
    pub yes_dollars: Vec<KalshiPriceLevel>,
    /// NO bid levels, best first.
    pub no_dollars: Vec<KalshiPriceLevel>,
}

/// A response carrying a market's order book.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderbookResponse {
    /// The order book with fixed-point contract counts in every level.
    pub orderbook_fp: KalshiOrderbook,
}

/// One public trade on a market.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiTrade {
    /// The unique identifier of the trade.
    pub trade_id: String,
    /// The market the trade occurred on.
    pub ticker: String,
    /// The traded contract count.
    pub count_fp: String,
    /// The YES price of the trade, in fixed-point dollars.
    pub yes_price_dollars: String,
    /// The NO price of the trade, in fixed-point dollars.
    pub no_price_dollars: String,
    /// The outcome side the taker is positioned for.
    pub taker_outcome_side: KalshiOutcomeSide,
    /// The book side of the taker, redundant with `taker_outcome_side`.
    #[serde(default)]
    pub taker_book_side: Option<KalshiBookSide>,
    /// The deprecated taker side, superseded by `taker_outcome_side`.
    #[serde(default)]
    pub taker_side: Option<KalshiOutcomeSide>,
    /// The time the trade executed.
    pub created_time: String,
    /// Whether the trade was matched off the book as a block trade.
    pub is_block_trade: bool,
}

/// A response carrying many trades and a pagination cursor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiTradesResponse {
    /// The trades on this page.
    pub trades: Vec<KalshiTrade>,
    /// The cursor for the next page, empty on the last page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// The status of the exchange.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiExchangeStatus {
    /// Whether the exchange accepts state changes at all.
    pub exchange_active: bool,
    /// Whether trading is permitted right now.
    pub trading_active: bool,
}

/// One exchange shard's balance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiIndexedBalance {
    /// The exchange shard.
    pub exchange_index: i64,
    /// The shard's balance, in fixed-point dollars.
    pub balance: String,
}

/// A member's balance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiBalanceResponse {
    /// The available balance in cents.
    pub balance: i64,
    /// The available balance in fixed-point dollars.
    pub balance_dollars: String,
    /// The portfolio value in cents.
    pub portfolio_value: i64,
    /// Unix timestamp of the last update to the balance.
    pub updated_ts: i64,
    /// The balance per exchange shard, omitted for subaccount-restricted keys.
    #[serde(default)]
    pub balance_breakdown: Vec<KalshiIndexedBalance>,
}

/// The cutoff timestamps that separate the venue's live and historical data tiers.
///
/// Orders, fills, and settled positions older than the relevant cutoff are only available from the
/// `/historical` endpoints, which this adapter does not read.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KalshiHistoricalCutoff {
    /// Settled markets older than this are only available from `GET /historical/markets`.
    #[serde(default)]
    pub market_settled_ts: Option<String>,
    /// Fills older than this are only available from `GET /historical/fills`.
    #[serde(default)]
    pub trades_created_ts: Option<String>,
    /// Canceled or executed orders older than this are only available from `GET /historical/orders`.
    #[serde(default)]
    pub orders_updated_ts: Option<String>,
    /// Settled positions older than this are only available from `GET /historical/positions`.
    #[serde(default)]
    pub market_positions_last_updated_ts: Option<String>,
}

/// A member's position in one market.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarketPosition {
    /// The market ticker.
    pub ticker: String,
    /// The exchange shard holding the position.
    pub exchange_index: i64,
    /// Total spent on this market, in fixed-point dollars.
    pub total_traded_dollars: String,
    /// Contracts held: positive is YES, negative is NO.
    pub position_fp: String,
    /// The cost of the aggregate position, in fixed-point dollars.
    pub market_exposure_dollars: String,
    /// Realized profit and loss, in fixed-point dollars.
    pub realized_pnl_dollars: String,
    /// Fees paid on fills, in fixed-point dollars.
    pub fees_paid_dollars: String,
    /// The last time the position changed.
    pub last_updated_ts: String,
}

/// A member's position across one event.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiEventPosition {
    /// The event ticker.
    pub event_ticker: String,
    /// Total spent on the event, in fixed-point dollars.
    pub total_cost_dollars: String,
    /// Shares traded on the event, YES and NO combined.
    pub total_cost_shares_fp: String,
    /// The cost of the aggregate event position, in fixed-point dollars.
    pub event_exposure_dollars: String,
    /// Realized profit and loss, in fixed-point dollars.
    pub realized_pnl_dollars: String,
    /// Fees paid on fills, in fixed-point dollars.
    pub fees_paid_dollars: String,
}

/// A member's positions, per market and per event.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiPositionsResponse {
    /// The markets the member holds contracts in.
    #[serde(default)]
    pub market_positions: Vec<KalshiMarketPosition>,
    /// The events the member holds contracts in.
    #[serde(default)]
    pub event_positions: Vec<KalshiEventPosition>,
    /// The cursor for the next page, empty on the last page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// A request to create one order.
///
/// The exchange expresses a two-sided market as book sides: `bid` buys the YES side and `ask` sells
/// it, so an order on a YES-quoted instrument carries the side of the book it touches.
#[derive(Clone, Debug, Serialize)]
pub struct KalshiCreateOrderRequest {
    /// The market ticker.
    pub ticker: String,
    /// An optional client-assigned identifier, which the exchange uses for idempotency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    /// The book side the order touches.
    pub side: KalshiBookSide,
    /// The order quantity as a fixed-point count string.
    pub count: String,
    /// The limit price as a fixed-point dollar string.
    pub price: String,
    /// The time the order stops trading, in Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_time: Option<i64>,
    /// How long the order remains open.
    pub time_in_force: KalshiTimeInForce,
    /// Whether the order may only rest, never trade.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    /// The self-trade prevention the exchange applies.
    pub self_trade_prevention_type: KalshiSelfTradePrevention,
    /// Whether the exchange cancels the order when trading pauses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_order_on_pause: Option<bool>,
    /// Whether the order may only reduce an existing position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
}

/// The exchange's answer to an order creation.
///
/// The response is a receipt, not the order: it reports the venue order identifier and the counts
/// immediately after the exchange accepted the request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiCreateOrderResponse {
    /// The venue order identifier.
    pub order_id: String,
    /// The client-assigned identifier the request carried, if any.
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// Contracts filled on acceptance, as a fixed-point count.
    pub fill_count: String,
    /// Contracts still working, as a fixed-point count.
    pub remaining_count: String,
    /// The average price of the contracts filled on acceptance, if any.
    #[serde(default)]
    pub average_fill_price: Option<String>,
    /// The average fee paid per filled contract on acceptance, if any.
    #[serde(default)]
    pub average_fee_paid: Option<String>,
    /// The exchange timestamp, in Unix milliseconds.
    pub ts_ms: i64,
}

/// The exchange's answer to an order cancellation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiCancelOrderResponse {
    /// The venue order identifier.
    pub order_id: String,
    /// The client-assigned identifier the order carried, if any.
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// Contracts canceled, as a fixed-point count.
    pub reduced_by: String,
    /// The exchange timestamp, in Unix milliseconds.
    pub ts_ms: i64,
}

/// A request to amend a resting order.
#[derive(Clone, Debug, Serialize)]
pub struct KalshiAmendOrderRequest {
    /// The market ticker.
    pub ticker: String,
    /// The book side the order touches.
    pub side: KalshiBookSide,
    /// The new limit price as a fixed-point dollar string.
    pub price: String,
    /// The new total count, which is the filled count plus the desired resting count.
    pub count: String,
    /// The client-assigned identifier the order carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    /// A replacement client-assigned identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_client_order_id: Option<String>,
}

/// The exchange's answer to an order amendment.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiAmendOrderResponse {
    /// The venue order identifier.
    pub order_id: String,
    /// The client-assigned identifier the order carries, if any.
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// Contracts still working after the amendment, as a fixed-point count.
    #[serde(default)]
    pub remaining_count: Option<String>,
    /// Contracts filled, as a fixed-point count.
    #[serde(default)]
    pub fill_count: Option<String>,
    /// The average fill price, if any contracts filled.
    #[serde(default)]
    pub average_fill_price: Option<String>,
    /// The average fee paid per filled contract, if any contracts filled.
    #[serde(default)]
    pub average_fee_paid: Option<String>,
    /// The exchange timestamp, in Unix milliseconds.
    pub ts_ms: i64,
}

/// A member's order, as the exchange reports it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrder {
    /// The venue order identifier.
    pub order_id: String,
    /// The member the order belongs to.
    #[serde(default)]
    pub user_id: Option<String>,
    /// The client-assigned identifier the order carries.
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// The market ticker.
    pub ticker: String,
    /// The outcome side the order is exposed to.
    pub outcome_side: KalshiOrderSide,
    /// The book side the order touches.
    pub book_side: KalshiBookSide,
    /// The order type.
    pub r#type: KalshiOrderType,
    /// The lifecycle state, left as a string so a new exchange state cannot fail the whole response.
    pub status: String,
    /// The YES price of the order, as a fixed-point dollar string.
    pub yes_price_dollars: String,
    /// The NO price of the order, as a fixed-point dollar string.
    pub no_price_dollars: String,
    /// Contracts filled, as a fixed-point count string.
    pub fill_count_fp: String,
    /// Contracts still working, as a fixed-point count string.
    pub remaining_count_fp: String,
    /// Contracts the order was created with, as a fixed-point count string.
    pub initial_count_fp: String,
    /// The cost of the taker fills, in fixed-point dollars.
    #[serde(default)]
    pub taker_fill_cost_dollars: Option<String>,
    /// The cost of the maker fills, in fixed-point dollars.
    #[serde(default)]
    pub maker_fill_cost_dollars: Option<String>,
    /// Fees paid on taker fills, in fixed-point dollars.
    #[serde(default)]
    pub taker_fees_dollars: Option<String>,
    /// Fees paid on maker fills, in fixed-point dollars.
    #[serde(default)]
    pub maker_fees_dollars: Option<String>,
    /// The time the order stops trading.
    #[serde(default)]
    pub expiration_time: Option<String>,
    /// The time the exchange accepted the order.
    #[serde(default)]
    pub created_time: Option<String>,
    /// The time of the last change to the order.
    #[serde(default)]
    pub last_update_time: Option<String>,
    /// The self-trade prevention the exchange applied.
    #[serde(default)]
    pub self_trade_prevention_type: Option<KalshiSelfTradePrevention>,
    /// Whether the exchange cancels the order when trading pauses.
    #[serde(default)]
    pub cancel_order_on_pause: Option<bool>,
    /// The subaccount the order belongs to.
    #[serde(default)]
    pub subaccount_number: Option<i64>,
}

/// A response carrying a single order.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderResponse {
    /// The order.
    pub order: KalshiOrder,
}

/// A page of orders.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrdersResponse {
    /// The orders on this page.
    #[serde(default)]
    pub orders: Vec<KalshiOrder>,
    /// The cursor for the next page, empty on the last page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// One fill of a member's order.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiFill {
    /// The fill identifier.
    pub fill_id: String,
    /// The trade identifier, which equals [`Self::fill_id`].
    pub trade_id: String,
    /// The venue order identifier the fill belongs to.
    pub order_id: String,
    /// The market ticker.
    pub ticker: String,
    /// The outcome side the fill is exposed to.
    pub outcome_side: KalshiOrderSide,
    /// The book side the fill touched.
    pub book_side: KalshiBookSide,
    /// Contracts filled, as a fixed-point count string.
    pub count_fp: String,
    /// The YES price of the fill, as a fixed-point dollar string.
    pub yes_price_dollars: String,
    /// The NO price of the fill, as a fixed-point dollar string.
    pub no_price_dollars: String,
    /// Whether the fill was the taker.
    pub is_taker: bool,
    /// The fee paid on the fill, in fixed-point dollars.
    #[serde(default)]
    pub fee_cost: Option<String>,
    /// The time the fill happened.
    #[serde(default)]
    pub created_time: Option<String>,
}

/// A page of fills.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiFillsResponse {
    /// The fills on this page.
    #[serde(default)]
    pub fills: Vec<KalshiFill>,
    /// The cursor for the next page, empty on the last page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// A request to create several orders in one call.
#[derive(Clone, Debug, Serialize)]
pub struct KalshiBatchCreateOrdersRequest {
    /// The orders to create.
    pub orders: Vec<KalshiCreateOrderRequest>,
}

/// The result of one order in a batched creation, which may carry an error instead of a receipt.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiBatchCreateOrdersItem {
    /// The venue order identifier, present when the order was accepted.
    #[serde(default)]
    pub order_id: Option<String>,
    /// The client-assigned identifier the order carried.
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// Contracts filled on acceptance.
    #[serde(default)]
    pub fill_count: Option<String>,
    /// Contracts still working.
    #[serde(default)]
    pub remaining_count: Option<String>,
    /// The average price of the contracts filled on acceptance, if any.
    #[serde(default)]
    pub average_fill_price: Option<String>,
    /// The average fee paid per filled contract, if any.
    #[serde(default)]
    pub average_fee_paid: Option<String>,
    /// The exchange timestamp, in Unix milliseconds.
    #[serde(default)]
    pub ts_ms: Option<i64>,
    /// The reason the order was refused, present when the exchange rejected it.
    #[serde(default)]
    pub error: Option<KalshiApiErrorBody>,
}

/// The exchange's answer to a batched order creation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiBatchCreateOrdersResponse {
    /// One result per order, in the order they were submitted.
    #[serde(default)]
    pub orders: Vec<KalshiBatchCreateOrdersItem>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::http::fixtures::{MARKET_JSON, market};

    #[rstest]
    fn test_market_decodes_documented_payload() {
        let market = market();

        assert_eq!(market.ticker, "KXHIGHNY-25JAN01-T50");
        assert_eq!(market.event_ticker, "KXHIGHNY-25JAN01");
        assert_eq!(market.market_type, KalshiMarketType::Binary);
        assert_eq!(market.status, KalshiMarketStatus::Active);
        assert_eq!(market.result, KalshiMarketResult::None);
        assert_eq!(market.notional_value_dollars, "1.0000");
        assert_eq!(market.yes_bid_dollars, "0.3400");
        assert_eq!(market.yes_ask_size_fp, "80.00");
        assert_eq!(market.price_ranges.len(), 1);
        assert_eq!(market.price_ranges[0].step, "0.0100");
        assert!(market.settlement_ts.is_none());
    }

    #[rstest]
    fn test_market_tolerates_unknown_fields() {
        let raw = MARKET_JSON.replace(
            "\"can_close_early\": true,",
            "\"can_close_early\": true, \"brand_new_field\": {\"x\": 1},",
        );
        let market: KalshiMarket = serde_json::from_str(&raw).expect("unknown fields are ignored");

        assert_eq!(market.ticker, "KXHIGHNY-25JAN01-T50");
    }

    #[rstest]
    fn test_market_reads_a_settled_outcome() {
        let raw = MARKET_JSON
            .replace("\"status\": \"active\"", "\"status\": \"finalized\"")
            .replace("\"result\": \"\"", "\"result\": \"yes\"")
            .replace(
                "\"expiration_value\": \"51\",",
                "\"expiration_value\": \"51\", \"settlement_value_dollars\": \"1.0000\", \"settlement_ts\": \"2025-01-02T06:00:00Z\",",
            );
        let market: KalshiMarket = serde_json::from_str(&raw).unwrap();

        assert!(market.status.is_final());
        assert!(market.result.is_yes());
        assert_eq!(market.settlement_value_dollars.as_deref(), Some("1.0000"));
        assert_eq!(
            market.settlement_ts.as_deref(),
            Some("2025-01-02T06:00:00Z")
        );
    }

    #[rstest]
    fn test_orderbook_decodes_price_levels() {
        let raw = r#"{
            "orderbook_fp": {
                "yes_dollars": [["0.3400", "120.00"], ["0.3300", "40.00"]],
                "no_dollars": [["0.6500", "60.00"]]
            }
        }"#;
        let response: KalshiOrderbookResponse = serde_json::from_str(raw).unwrap();

        assert_eq!(
            response.orderbook_fp.yes_dollars[0],
            ("0.3400".to_string(), "120.00".to_string())
        );
        assert_eq!(response.orderbook_fp.no_dollars.len(), 1);
    }

    #[rstest]
    fn test_event_decodes_mutual_exclusivity_and_nested_markets() {
        let raw = format!(
            r#"{{
                "event": {{
                    "event_ticker": "KXHIGHNY-25JAN01",
                    "series_ticker": "KXHIGHNY",
                    "sub_title": "Highest temperature in NYC",
                    "title": "Highest temperature in NYC on Jan 1, 2025",
                    "collateral_return_type": "binary",
                    "mutually_exclusive": true,
                    "last_updated_ts": "2024-12-30T15:00:00Z"
                }},
                "markets": [{MARKET_JSON}]
            }}"#
        );
        let response: KalshiEventResponse = serde_json::from_str(&raw).unwrap();

        assert!(response.event.mutually_exclusive);
        assert_eq!(response.event.series_ticker, "KXHIGHNY");
        // Markets are returned as a top-level field unless the request nests them.
        assert!(response.event.markets.is_empty());
        assert_eq!(response.markets.len(), 1);
        assert_eq!(response.markets[0].ticker, "KXHIGHNY-25JAN01-T50");
    }

    #[rstest]
    fn test_trade_prefers_the_outcome_side_over_the_deprecated_side() {
        let raw = r#"{
            "trade_id": "9f2b2b0e-1c1a-4b0e-9f7a-2b6a5b1c9d10",
            "ticker": "KXHIGHNY-25JAN01-T50",
            "count_fp": "25.00",
            "yes_price_dollars": "0.3500",
            "no_price_dollars": "0.6500",
            "taker_side": "yes",
            "taker_outcome_side": "no",
            "taker_book_side": "ask",
            "created_time": "2025-01-01T12:00:00Z",
            "is_block_trade": false
        }"#;
        let trade: KalshiTrade = serde_json::from_str(raw).unwrap();

        assert_eq!(trade.taker_outcome_side, KalshiOutcomeSide::No);
        assert_eq!(trade.taker_book_side, Some(KalshiBookSide::Ask));
        assert_eq!(trade.taker_side, Some(KalshiOutcomeSide::Yes));
        assert!(!trade.is_block_trade);
    }

    #[rstest]
    fn test_balance_and_positions_decode_documented_payloads() {
        let balance: KalshiBalanceResponse = serde_json::from_str(
            r#"{
                "balance": 412500,
                "balance_dollars": "4125.0000",
                "portfolio_value": 500000,
                "updated_ts": 1735732800,
                "balance_breakdown": [{"exchange_index": 0, "balance": "4125.0000"}]
            }"#,
        )
        .unwrap();
        let positions: KalshiPositionsResponse = serde_json::from_str(
            r#"{
                "cursor": "",
                "market_positions": [{
                    "ticker": "KXHIGHNY-25JAN01-T50",
                    "exchange_index": 0,
                    "total_traded_dollars": "350.0000",
                    "position_fp": "100.00",
                    "market_exposure_dollars": "350.0000",
                    "realized_pnl_dollars": "0.0000",
                    "fees_paid_dollars": "1.7500",
                    "last_updated_ts": "2025-01-01T12:00:00Z"
                }],
                "event_positions": [{
                    "event_ticker": "KXHIGHNY-25JAN01",
                    "total_cost_dollars": "350.0000",
                    "total_cost_shares_fp": "100.00",
                    "event_exposure_dollars": "350.0000",
                    "realized_pnl_dollars": "0.0000",
                    "fees_paid_dollars": "1.7500"
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(balance.balance, 412_500);
        assert_eq!(balance.balance_breakdown.len(), 1);
        assert_eq!(positions.market_positions[0].position_fp, "100.00");
        assert_eq!(
            positions.event_positions[0].event_ticker,
            "KXHIGHNY-25JAN01"
        );
    }
}

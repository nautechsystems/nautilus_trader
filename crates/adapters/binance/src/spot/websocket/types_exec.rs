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

//! Binance Spot User Data Stream JSON types.
//!
//! These types represent the JSON events pushed by Binance after subscribing
//! via `userDataStream.subscribe.signature` on the WebSocket API. All events
//! arrive as JSON text frames (not SBE binary), verified empirically.
//!
//! Push events use a wrapper format: `{"subscriptionId": N, "event": {...}}`

use serde::Deserialize;

use crate::common::enums::{BinanceOrderStatus, BinanceSide, BinanceTimeInForce};
use crate::spot::enums::BinanceSpotOrderType;

/// Wrapper for all User Data Stream push events from Binance Spot.
///
/// All push events arrive in this envelope format after subscribing via
/// `userDataStream.subscribe.signature`. The inner `event` is deserialized
/// directly to the correct variant using the `"e"` tag in a single pass.
#[derive(Debug, Clone, Deserialize)]
pub struct UserDataStreamFrame {
    /// Subscription identifier returned by the subscribe response.
    #[serde(rename = "subscriptionId")]
    pub subscription_id: u64,
    /// The typed event payload, dispatched by the `"e"` field.
    pub event: UserDataStreamEvent,
}

/// Tagged enum for User Data Stream event types.
///
/// Serde routes to the correct variant based on the `"e"` field in a single
/// deserialization pass, avoiding double-parsing through `serde_json::Value`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "e")]
pub enum UserDataStreamEvent {
    /// Execution report (order lifecycle events).
    #[serde(rename = "executionReport")]
    ExecutionReport(Box<BinanceSpotExecutionReport>),
    /// Account position update (balance changes).
    #[serde(rename = "outboundAccountPosition")]
    AccountPosition(BinanceSpotAccountPosition),
    /// Balance update (deposits, withdrawals — logged only).
    #[serde(rename = "balanceUpdate")]
    BalanceUpdate(serde_json::Value),
    /// Unknown event type.
    #[serde(other)]
    Unknown,
}

/// Execution type for Binance Spot `executionReport` events.
///
/// Spot-specific variant of execution types. Includes `Rejected` and
/// `TradePrevention` which are not present in the Futures adapter.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceSpotExecutionType {
    /// New order accepted by the matching engine.
    New,
    /// Order canceled (by user or exchange).
    Canceled,
    /// Order replaced (cancel-replace).
    Replaced,
    /// Order rejected after initial acceptance.
    Rejected,
    /// Trade (partial or full fill).
    Trade,
    /// Order expired (TIF, FOK, etc.).
    Expired,
    /// Self-trade prevention triggered.
    TradePrevention,
    /// Unknown or undocumented execution type.
    #[serde(other)]
    Unknown,
}

/// Execution report event from Binance Spot User Data Stream.
///
/// Represents all order lifecycle events: acceptance, fills, cancellations,
/// rejections, and expirations. Field names follow Binance's single-letter
/// JSON convention.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotExecutionReport {
    // Note: the `"e"` field is consumed by the `UserDataStreamEvent` tag
    // and is not present here. It is always `"executionReport"` when this
    // struct is deserialized via the tagged enum.
    /// Event time in milliseconds since epoch.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Trading symbol (e.g., `"ETHUSDC"`).
    #[serde(rename = "s")]
    pub symbol: String,
    /// Client order ID assigned by the submitter.
    #[serde(rename = "c")]
    pub client_order_id: String,
    /// Order side.
    #[serde(rename = "S")]
    pub side: BinanceSide,
    /// Order type.
    #[serde(rename = "o")]
    pub order_type: BinanceSpotOrderType,
    /// Time in force.
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Original order quantity.
    #[serde(rename = "q")]
    pub orig_qty: String,
    /// Order price (for limit orders).
    #[serde(rename = "p")]
    pub price: String,
    /// Stop price.
    #[serde(rename = "P")]
    pub stop_price: String,
    /// Iceberg quantity.
    #[serde(rename = "F")]
    pub iceberg_qty: String,
    /// Order list ID (for OCO orders, -1 if not OCO).
    #[serde(rename = "g")]
    pub order_list_id: i64,
    /// Original client order ID (for cancel/replace, the original order's ID).
    #[serde(rename = "C")]
    pub orig_client_order_id: String,
    /// Current execution type.
    #[serde(rename = "x")]
    pub execution_type: BinanceSpotExecutionType,
    /// Current order status.
    #[serde(rename = "X")]
    pub order_status: BinanceOrderStatus,
    /// Order rejection reason (or `"NONE"`).
    #[serde(rename = "r")]
    pub reject_reason: String,
    /// Binance-assigned order ID (venue order ID).
    #[serde(rename = "i")]
    pub order_id: i64,
    /// Last executed quantity (this fill only).
    #[serde(rename = "l")]
    pub last_qty: String,
    /// Cumulative filled quantity.
    #[serde(rename = "z")]
    pub cumulative_filled_qty: String,
    /// Last executed price (this fill only).
    #[serde(rename = "L")]
    pub last_price: String,
    /// Commission amount.
    #[serde(rename = "n")]
    pub commission: String,
    /// Commission asset (e.g., `"ETH"`, `"BNB"`). Null if no fill.
    #[serde(rename = "N", default)]
    pub commission_asset: Option<String>,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Trade ID (-1 if not a fill).
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// Is the order on the book?
    #[serde(rename = "w")]
    pub is_working: bool,
    /// Is this a maker trade?
    #[serde(rename = "m")]
    pub is_maker: bool,
    /// Order creation time in milliseconds.
    #[serde(rename = "O")]
    pub order_creation_time: i64,
    /// Cumulative quote quantity (cost).
    #[serde(rename = "Z")]
    pub cumulative_quote_qty: String,
    /// Last quote quantity.
    #[serde(rename = "Y")]
    pub last_quote_qty: String,
    /// Quote order quantity (for MARKET orders with quoteOrderQty).
    #[serde(rename = "Q")]
    pub quote_order_qty: String,
    /// Working time in milliseconds.
    #[serde(rename = "W", default)]
    pub working_time: Option<i64>,
    /// Self-trade prevention mode.
    #[serde(rename = "V", default)]
    pub self_trade_prevention_mode: Option<String>,
}

/// Account balance entry from `outboundAccountPosition` event.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotBalance {
    /// Asset symbol (e.g., `"USDC"`, `"ETH"`).
    #[serde(rename = "a")]
    pub asset: String,
    /// Free (available) balance.
    #[serde(rename = "f")]
    pub free: String,
    /// Locked balance (in open orders).
    #[serde(rename = "l")]
    pub locked: String,
}

/// Account position event from Binance Spot User Data Stream.
///
/// Emitted whenever account balances change (order placed, fill, deposit, etc.).
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotAccountPosition {
    // Note: the `"e"` field is consumed by the `UserDataStreamEvent` tag.
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Time of last account update in milliseconds.
    #[serde(rename = "u")]
    pub update_time: i64,
    /// Account balances that changed.
    #[serde(rename = "B")]
    pub balances: Vec<BinanceSpotBalance>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Real JSON captured from live Binance WS API.
    const FRAME_EXECUTION_REPORT_NEW: &str = r#"{
        "subscriptionId": 0,
        "event": {
            "e": "executionReport", "E": 1772494856997, "s": "ETHUSDC",
            "c": "UDS-TEST-1772494856", "S": "BUY", "o": "LIMIT", "f": "GTC",
            "q": "0.01000000", "p": "1500.00000000", "P": "0.00000000",
            "F": "0.00000000", "g": -1, "C": "", "x": "NEW", "X": "NEW",
            "r": "NONE", "i": 9399999776, "l": "0.00000000", "z": "0.00000000",
            "L": "0.00000000", "n": "0", "N": null, "T": 1772494856997,
            "t": -1, "w": true, "m": false, "O": 1772494856997,
            "Z": "0.00000000", "Y": "0.00000000", "Q": "0.00000000",
            "W": 1772494856997, "V": "EXPIRE_MAKER"
        }
    }"#;

    const EXECUTION_REPORT_NEW: &str = r#"{
        "e": "executionReport", "E": 1772494856997, "s": "ETHUSDC",
        "c": "UDS-TEST-1772494856", "S": "BUY", "o": "LIMIT", "f": "GTC",
        "q": "0.01000000", "p": "1500.00000000", "P": "0.00000000",
        "F": "0.00000000", "g": -1, "C": "", "x": "NEW", "X": "NEW",
        "r": "NONE", "i": 9399999776, "l": "0.00000000", "z": "0.00000000",
        "L": "0.00000000", "n": "0", "N": null, "T": 1772494856997,
        "t": -1, "w": true, "m": false, "O": 1772494856997,
        "Z": "0.00000000", "Y": "0.00000000", "Q": "0.00000000",
        "W": 1772494856997, "V": "EXPIRE_MAKER"
    }"#;

    const EXECUTION_REPORT_TRADE: &str = r#"{
        "e": "executionReport", "E": 1772494860000, "s": "ETHUSDC",
        "c": "UDS-TEST-001", "S": "BUY", "o": "LIMIT", "f": "GTC",
        "q": "0.01000000", "p": "2045.50000000", "P": "0.00000000",
        "F": "0.00000000", "g": -1, "C": "", "x": "TRADE", "X": "FILLED",
        "r": "NONE", "i": 9399999776, "l": "0.01000000", "z": "0.01000000",
        "L": "2045.50000000", "n": "0.00001234", "N": "ETH",
        "T": 1772494860000, "t": 123456789, "w": false, "m": true,
        "O": 1772494856997, "Z": "20.45500000", "Y": "20.45500000",
        "Q": "0.00000000", "W": 1772494856997, "V": "EXPIRE_MAKER"
    }"#;

    const EXECUTION_REPORT_CANCELED: &str = r#"{
        "e": "executionReport", "E": 1772494873278, "s": "ETHUSDC",
        "c": "PAyFKkUBxfnY0fqEogcln5", "S": "BUY", "o": "LIMIT", "f": "GTC",
        "q": "0.01000000", "p": "1500.00000000", "P": "0.00000000",
        "F": "0.00000000", "g": -1, "C": "UDS-TEST-1772494856",
        "x": "CANCELED", "X": "CANCELED", "r": "NONE", "i": 9399999776,
        "l": "0.00000000", "z": "0.00000000", "L": "0.00000000",
        "n": "0", "N": null, "T": 1772494873278, "t": -1, "w": false,
        "m": false, "O": 1772494856997, "Z": "0.00000000",
        "Y": "0.00000000", "Q": "0.00000000", "V": "EXPIRE_MAKER"
    }"#;

    const ACCOUNT_POSITION: &str = r#"{
        "e": "outboundAccountPosition", "E": 1772494856997,
        "u": 1772494856997,
        "B": [
            {"a": "ETH", "f": "0.14741228", "l": "0.00000000"},
            {"a": "BNB", "f": "0.00000000", "l": "0.00000000"},
            {"a": "USDC", "f": "886.63366221", "l": "15.00000000"}
        ]
    }"#;

    #[rstest]
    fn deserialize_uds_frame_single_pass() {
        let frame: UserDataStreamFrame = serde_json::from_str(FRAME_EXECUTION_REPORT_NEW)
            .expect("Failed to deserialize UDS frame");
        assert_eq!(frame.subscription_id, 0);
        match frame.event {
            UserDataStreamEvent::ExecutionReport(report) => {
                assert_eq!(report.symbol, "ETHUSDC");
                assert_eq!(report.execution_type, BinanceSpotExecutionType::New);
            }
            other => panic!("Expected ExecutionReport, got {other:?}"),
        }
    }

    #[rstest]
    fn deserialize_uds_frame_account_position() {
        let json = format!(
            r#"{{"subscriptionId": 0, "event": {ACCOUNT_POSITION}}}"#,
        );
        let frame: UserDataStreamFrame =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(frame.subscription_id, 0);
        match frame.event {
            UserDataStreamEvent::AccountPosition(pos) => {
                assert_eq!(pos.balances.len(), 3);
            }
            other => panic!("Expected AccountPosition, got {other:?}"),
        }
    }

    #[rstest]
    fn deserialize_uds_frame_unknown_event() {
        let json = r#"{"subscriptionId": 0, "event": {"e": "listStatus", "E": 1772494860000}}"#;
        let frame: UserDataStreamFrame =
            serde_json::from_str(json).expect("Failed to deserialize");
        assert!(matches!(frame.event, UserDataStreamEvent::Unknown));
    }

    #[rstest]
    fn deserialize_execution_report_new() {
        let report: BinanceSpotExecutionReport =
            serde_json::from_str(EXECUTION_REPORT_NEW).expect("Failed to deserialize NEW");
        assert_eq!(report.symbol, "ETHUSDC");
        assert_eq!(report.client_order_id, "UDS-TEST-1772494856");
        assert_eq!(report.execution_type, BinanceSpotExecutionType::New);
        assert_eq!(report.side, BinanceSide::Buy);
        assert_eq!(report.order_type, BinanceSpotOrderType::Limit);
        assert_eq!(report.order_id, 9399999776);
        assert!(report.commission_asset.is_none());
        assert_eq!(report.trade_id, -1);
        assert!(report.is_working);
        assert!(!report.is_maker);
    }

    #[rstest]
    fn deserialize_execution_report_trade_fill() {
        let report: BinanceSpotExecutionReport =
            serde_json::from_str(EXECUTION_REPORT_TRADE).expect("Failed to deserialize TRADE");
        assert_eq!(report.execution_type, BinanceSpotExecutionType::Trade);
        assert_eq!(report.order_status, BinanceOrderStatus::Filled);
        assert_eq!(report.last_qty, "0.01000000");
        assert_eq!(report.last_price, "2045.50000000");
        assert_eq!(report.cumulative_filled_qty, "0.01000000");
        assert_eq!(report.commission, "0.00001234");
        assert_eq!(report.commission_asset.as_deref(), Some("ETH"));
        assert_eq!(report.trade_id, 123456789);
        assert!(report.is_maker);
        assert!(!report.is_working);
    }

    #[rstest]
    fn deserialize_execution_report_canceled() {
        let report: BinanceSpotExecutionReport = serde_json::from_str(EXECUTION_REPORT_CANCELED)
            .expect("Failed to deserialize CANCELED");
        assert_eq!(report.execution_type, BinanceSpotExecutionType::Canceled);
        assert_eq!(report.order_status, BinanceOrderStatus::Canceled);
        assert_eq!(report.orig_client_order_id, "UDS-TEST-1772494856");
        assert_eq!(report.trade_id, -1);
    }

    #[rstest]
    fn deserialize_account_position() {
        let position: BinanceSpotAccountPosition =
            serde_json::from_str(ACCOUNT_POSITION).expect("Failed to deserialize account position");
        // event_type ("e") is consumed by the UserDataStreamEvent tag
        assert_eq!(position.balances.len(), 3);
        assert_eq!(position.balances[0].asset, "ETH");
        assert_eq!(position.balances[0].free, "0.14741228");
        assert_eq!(position.balances[0].locked, "0.00000000");
        assert_eq!(position.balances[2].asset, "USDC");
        assert_eq!(position.balances[2].free, "886.63366221");
        assert_eq!(position.balances[2].locked, "15.00000000");
    }

    #[rstest]
    fn parse_fill_values_to_f64() {
        let report: BinanceSpotExecutionReport =
            serde_json::from_str(EXECUTION_REPORT_TRADE).expect("Failed to deserialize");
        let last_qty: f64 = report.last_qty.parse().expect("Failed to parse last_qty");
        let last_price: f64 = report
            .last_price
            .parse()
            .expect("Failed to parse last_price");
        let commission: f64 = report
            .commission
            .parse()
            .expect("Failed to parse commission");
        assert!((last_qty - 0.01).abs() < 1e-10);
        assert!((last_price - 2045.5).abs() < 1e-10);
        assert!((commission - 0.00001234).abs() < 1e-12);
    }

    #[rstest]
    fn deserialize_execution_type_variants() {
        let test_cases = [
            (r#""NEW""#, BinanceSpotExecutionType::New),
            (r#""CANCELED""#, BinanceSpotExecutionType::Canceled),
            (r#""REPLACED""#, BinanceSpotExecutionType::Replaced),
            (r#""REJECTED""#, BinanceSpotExecutionType::Rejected),
            (r#""TRADE""#, BinanceSpotExecutionType::Trade),
            (r#""EXPIRED""#, BinanceSpotExecutionType::Expired),
            (
                r#""TRADE_PREVENTION""#,
                BinanceSpotExecutionType::TradePrevention,
            ),
        ];
        for (json, expected) in test_cases {
            let result: BinanceSpotExecutionType =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("Failed for {json}: {e}"));
            assert_eq!(result, expected, "Mismatch for {json}");
        }
    }

    #[rstest]
    fn deserialize_unknown_execution_type() {
        let result: BinanceSpotExecutionType =
            serde_json::from_str(r#""INSURANCE_FUND""#).expect("Should not fail");
        assert_eq!(result, BinanceSpotExecutionType::Unknown);
    }

    #[rstest]
    fn deserialize_typed_enum_fields_from_json() {
        let report: BinanceSpotExecutionReport =
            serde_json::from_str(EXECUTION_REPORT_TRADE).expect("Failed to deserialize");
        assert_eq!(report.side, BinanceSide::Buy);
        assert_eq!(report.order_type, BinanceSpotOrderType::Limit);
        assert_eq!(report.time_in_force, BinanceTimeInForce::Gtc);
        assert_eq!(report.order_status, BinanceOrderStatus::Filled);
    }
}

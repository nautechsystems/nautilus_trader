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

//! Binance Spot User Data Stream feed handler.
//!
//! Receives raw WebSocket text frames, routes by event type, and deserializes
//! into venue-specific message types. No Nautilus model imports.

use tokio_tungstenite::tungstenite::Message;

use super::messages::{
    BinanceSpotAccountPositionMsg, BinanceSpotBalanceUpdateMsg, BinanceSpotExecutionReport,
    BinanceSpotUdsMessage,
};

/// Parses a raw WebSocket message into a typed UDS message.
///
/// Routes based on the `"e"` (event type) field in the JSON payload.
/// Returns `None` for non-text frames (ping/pong/close) or unrecognized events.
pub fn parse_raw_message(msg: Message) -> Option<BinanceSpotUdsMessage> {
    let text = match msg {
        Message::Text(text) => text,
        Message::Ping(_) | Message::Pong(_) => return None,
        Message::Close(_) => {
            log::debug!("WebSocket close frame received");
            return None;
        }
        Message::Binary(data) => {
            log::warn!(
                "Unexpected binary frame on user data stream ({} bytes)",
                data.len()
            );
            return None;
        }
        Message::Frame(_) => return None,
    };

    let value: serde_json::Value = match serde_json::from_str(text.as_str()) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Failed to parse UDS JSON: {e}");
            return None;
        }
    };

    let event_type = value.get("e")?.as_str()?;

    match event_type {
        "executionReport" => {
            let report: BinanceSpotExecutionReport = serde_json::from_value(value)
                .map_err(|e| {
                    log::warn!("Failed to deserialize executionReport: {e}");
                    e
                })
                .ok()?;
            Some(BinanceSpotUdsMessage::ExecutionReport(Box::new(report)))
        }
        "outboundAccountPosition" => {
            let msg: BinanceSpotAccountPositionMsg = serde_json::from_value(value)
                .map_err(|e| {
                    log::warn!("Failed to deserialize outboundAccountPosition: {e}");
                    e
                })
                .ok()?;
            Some(BinanceSpotUdsMessage::AccountPosition(msg))
        }
        "balanceUpdate" => {
            let msg: BinanceSpotBalanceUpdateMsg = serde_json::from_value(value)
                .map_err(|e| {
                    log::warn!("Failed to deserialize balanceUpdate: {e}");
                    e
                })
                .ok()?;
            Some(BinanceSpotUdsMessage::BalanceUpdate(msg))
        }
        "listenKeyExpired" => {
            log::warn!("Listen key expired");
            Some(BinanceSpotUdsMessage::ListenKeyExpired)
        }
        other => {
            log::debug!("Unhandled UDS event type: {other}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;

    #[rstest]
    fn test_parse_execution_report_new() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_new.json");
        let msg = Message::Text(json.to_string().into());
        let result = parse_raw_message(msg);

        assert!(matches!(
            result,
            Some(BinanceSpotUdsMessage::ExecutionReport(_))
        ));
    }

    #[rstest]
    fn test_parse_execution_report_trade() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_trade.json");
        let msg = Message::Text(json.to_string().into());
        let result = parse_raw_message(msg);

        assert!(matches!(
            result,
            Some(BinanceSpotUdsMessage::ExecutionReport(_))
        ));
    }

    #[rstest]
    fn test_parse_account_position() {
        let json = include_str!("../../../../test_data/ws_spot_account_position.json");
        let msg = Message::Text(json.to_string().into());
        let result = parse_raw_message(msg);

        assert!(matches!(
            result,
            Some(BinanceSpotUdsMessage::AccountPosition(_))
        ));
    }

    #[rstest]
    fn test_parse_balance_update() {
        let json = include_str!("../../../../test_data/ws_spot_balance_update.json");
        let msg = Message::Text(json.to_string().into());
        let result = parse_raw_message(msg);

        assert!(matches!(
            result,
            Some(BinanceSpotUdsMessage::BalanceUpdate(_))
        ));
    }

    #[rstest]
    fn test_parse_listen_key_expired() {
        let json = r#"{"e": "listenKeyExpired", "E": 1709654400000}"#;
        let msg = Message::Text(json.to_string().into());
        let result = parse_raw_message(msg);

        assert!(matches!(
            result,
            Some(BinanceSpotUdsMessage::ListenKeyExpired)
        ));
    }

    #[rstest]
    fn test_parse_ping_returns_none() {
        let msg = Message::Ping(vec![].into());
        assert!(parse_raw_message(msg).is_none());
    }

    #[rstest]
    fn test_parse_unknown_event_returns_none() {
        let json = r#"{"e": "unknownEvent", "E": 1709654400000}"#;
        let msg = Message::Text(json.to_string().into());
        assert!(parse_raw_message(msg).is_none());
    }
}

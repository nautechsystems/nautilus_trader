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

//! Integration tests for the Kalshi WebSocket client.

use std::path::PathBuf;

use nautilus_kalshi::websocket::messages::KalshiWsMessage;

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_str(filename: &str) -> String {
    std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("missing: {filename}"))
}

#[test]
fn test_ws_message_parsing_snapshot() {
    let raw = load_str("ws_orderbook_snapshot.json");
    let msg = KalshiWsMessage::from_json(&raw).unwrap();
    assert!(matches!(msg, KalshiWsMessage::OrderbookSnapshot { .. }));
}

#[test]
fn test_ws_message_parsing_trade() {
    let raw = load_str("ws_trade.json");
    let msg = KalshiWsMessage::from_json(&raw).unwrap();
    assert!(matches!(msg, KalshiWsMessage::Trade { .. }));
}

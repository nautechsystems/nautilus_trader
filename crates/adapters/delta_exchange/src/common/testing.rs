// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Testing utilities for Delta Exchange integration.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::json;

use super::{
    enums::{
        DeltaExchangeAssetStatus, DeltaExchangeOrderState, DeltaExchangeOrderType,
        DeltaExchangeProductType, DeltaExchangeSide, DeltaExchangeTimeInForce,
        DeltaExchangeTradingState,
    },
    models::{
        DeltaExchangeAsset, DeltaExchangeBalance, DeltaExchangeCandle, DeltaExchangeFill,
        DeltaExchangeOrder, DeltaExchangeOrderBook, DeltaExchangeOrderBookLevel,
        DeltaExchangePosition, DeltaExchangeProduct, DeltaExchangeTicker, DeltaExchangeTrade,
    },
};

/// Create a test Delta Exchange asset.
pub fn test_asset() -> DeltaExchangeAsset {
    DeltaExchangeAsset {
        id: 1,
        symbol: "BTC".into(),
        name: "Bitcoin".to_string(),
        status: DeltaExchangeAssetStatus::Active,
        precision: 8,
        deposit_status: "enabled".to_string(),
        withdrawal_status: "enabled".to_string(),
        base_withdrawal_fee: Decimal::new(5, 4), // 0.0005
        min_withdrawal_amount: Decimal::new(1, 3), // 0.001
    }
}

/// Create a test Delta Exchange product.
pub fn test_product() -> DeltaExchangeProduct {
    DeltaExchangeProduct {
        id: 27,
        symbol: "BTCUSD".into(),
        description: "Bitcoin Perpetual".to_string(),
        product_type: DeltaExchangeProductType::PerpetualFutures,
        underlying_asset: Some("BTC".into()),
        quoting_asset: Some("USD".into()),
        settlement_asset: Some("USDT".into()),
        contract_value: Decimal::ONE,
        contract_unit_currency: "USD".to_string(),
        tick_size: Decimal::new(5, 1), // 0.5
        min_size: Decimal::ONE,
        max_size: Some(Decimal::new(1000000, 0)),
        state: DeltaExchangeTradingState::Operational,
        tradeable: true,
        launch_date: Some(DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z").unwrap().with_timezone(&Utc)),
        settlement_time: None,
        strike_price: None,
        initial_margin: Decimal::new(5, 2), // 0.05
        maintenance_margin: Decimal::new(25, 3), // 0.025
        maker_commission_rate: Decimal::new(2, 4), // 0.0002
        taker_commission_rate: Decimal::new(5, 4), // 0.0005
        liquidation_penalty_rate: Decimal::new(5, 3), // 0.005
    }
}

/// Create a test Delta Exchange order.
pub fn test_order() -> DeltaExchangeOrder {
    DeltaExchangeOrder {
        id: 12345,
        user_id: 1001,
        product_id: 27,
        product_symbol: "BTCUSD".into(),
        size: Decimal::new(100, 0),
        unfilled_size: Decimal::new(50, 0),
        side: DeltaExchangeSide::Buy,
        order_type: DeltaExchangeOrderType::LimitOrder,
        limit_price: Some(Decimal::new(50000, 0)),
        stop_price: None,
        paid_price: Some(Decimal::new(49950, 0)),
        state: DeltaExchangeOrderState::Open,
        time_in_force: DeltaExchangeTimeInForce::Gtc,
        post_only: false,
        reduce_only: false,
        client_order_id: Some("client-123".to_string()),
        created_at: DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z").unwrap().with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z").unwrap().with_timezone(&Utc),
    }
}

/// Create a test Delta Exchange position.
pub fn test_position() -> DeltaExchangePosition {
    DeltaExchangePosition {
        user_id: 1001,
        product_id: 27,
        product_symbol: "BTCUSD".into(),
        size: Decimal::new(100, 0),
        entry_price: Some(Decimal::new(50000, 0)),
        mark_price: Some(Decimal::new(51000, 0)),
        unrealized_pnl: Decimal::new(1000, 0),
        realized_pnl: Decimal::new(500, 0),
        margin: Decimal::new(2500, 0),
        maintenance_margin: Decimal::new(1250, 0),
        liquidation_price: Some(Decimal::new(47500, 0)),
        created_at: DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z").unwrap().with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z").unwrap().with_timezone(&Utc),
    }
}

/// Create a test Delta Exchange balance.
pub fn test_balance() -> DeltaExchangeBalance {
    DeltaExchangeBalance {
        asset_id: 1,
        asset_symbol: "USDT".into(),
        available_balance: Decimal::new(10000, 0),
        order_margin: Decimal::new(2500, 0),
        position_margin: Decimal::new(2500, 0),
        commission: Decimal::new(10, 0),
        withdrawal_pending: Decimal::ZERO,
        deposit_pending: Decimal::ZERO,
        balance: Decimal::new(15010, 0),
    }
}

/// Create a test Delta Exchange fill.
pub fn test_fill() -> DeltaExchangeFill {
    DeltaExchangeFill {
        id: 67890,
        user_id: 1001,
        order_id: 12345,
        product_id: 27,
        product_symbol: "BTCUSD".into(),
        size: Decimal::new(50, 0),
        price: Decimal::new(49950, 0),
        side: DeltaExchangeSide::Buy,
        commission: Decimal::new(25, 0),
        realized_pnl: Decimal::new(100, 0),
        created_at: DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z").unwrap().with_timezone(&Utc),
        role: "taker".to_string(),
        client_order_id: Some("client-123".to_string()),
    }
}

/// Create a test Delta Exchange ticker.
pub fn test_ticker() -> DeltaExchangeTicker {
    DeltaExchangeTicker {
        symbol: "BTCUSD".into(),
        price: Some(Decimal::new(50000, 0)),
        change_24h: Some(Decimal::new(1000, 0)),
        high_24h: Some(Decimal::new(51000, 0)),
        low_24h: Some(Decimal::new(49000, 0)),
        volume_24h: Some(Decimal::new(1000000, 0)),
        bid: Some(Decimal::new(49995, 0)),
        ask: Some(Decimal::new(50005, 0)),
        mark_price: Some(Decimal::new(50000, 0)),
        open_interest: Some(Decimal::new(5000000, 0)),
        timestamp: 1704110400000, // 2024-01-01T12:00:00Z in milliseconds
    }
}

/// Create a test Delta Exchange trade.
pub fn test_trade() -> DeltaExchangeTrade {
    DeltaExchangeTrade {
        id: 98765,
        symbol: "BTCUSD".into(),
        price: Decimal::new(50000, 0),
        size: Decimal::new(10, 0),
        buyer_role: "taker".to_string(),
        timestamp: 1704110400000, // 2024-01-01T12:00:00Z in milliseconds
    }
}

/// Create a test Delta Exchange candle.
pub fn test_candle() -> DeltaExchangeCandle {
    DeltaExchangeCandle {
        time: 1704110400, // 2024-01-01T12:00:00Z in seconds
        open: Decimal::new(49500, 0),
        high: Decimal::new(50500, 0),
        low: Decimal::new(49000, 0),
        close: Decimal::new(50000, 0),
        volume: Decimal::new(1000, 0),
    }
}

/// Create a test Delta Exchange order book.
pub fn test_orderbook() -> DeltaExchangeOrderBook {
    DeltaExchangeOrderBook {
        symbol: "BTCUSD".into(),
        buy: vec![
            DeltaExchangeOrderBookLevel {
                price: Decimal::new(49995, 0),
                size: Decimal::new(100, 0),
            },
            DeltaExchangeOrderBookLevel {
                price: Decimal::new(49990, 0),
                size: Decimal::new(200, 0),
            },
        ],
        sell: vec![
            DeltaExchangeOrderBookLevel {
                price: Decimal::new(50005, 0),
                size: Decimal::new(150, 0),
            },
            DeltaExchangeOrderBookLevel {
                price: Decimal::new(50010, 0),
                size: Decimal::new(250, 0),
            },
        ],
        last_sequence_no: 12345,
        last_updated_at: 1704110400000, // 2024-01-01T12:00:00Z in milliseconds
    }
}

/// Create test JSON response for successful API call.
pub fn test_success_response<T>(data: T) -> serde_json::Value
where
    T: serde::Serialize,
{
    json!({
        "success": true,
        "result": data
    })
}

/// Create test JSON response for API error.
pub fn test_error_response(code: &str, context: Option<serde_json::Value>) -> serde_json::Value {
    json!({
        "success": false,
        "error": {
            "code": code,
            "context": context
        }
    })
}

/// Create test credentials for testing.
pub fn test_credentials() -> (String, String) {
    (
        "test_api_key_123".to_string(),
        "test_api_secret_456".to_string(),
    )
}

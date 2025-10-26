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

use anyhow::Context;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::models::{PerpMeta, SpotMeta};

/// Market type enumeration for normalized instrument definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidMarketType {
    /// Perpetual futures contract.
    Perp,
    /// Spot trading pair.
    Spot,
}

/// Normalized instrument definition produced by this parser.
///
/// This deliberately avoids any tight coupling to Nautilus' Cython types.
/// The InstrumentProvider can later convert this into Nautilus `Instrument`s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidInstrumentDef {
    /// Human-readable symbol (e.g., "BTC-USD-PERP", "PURR-USDC-SPOT").
    pub symbol: String,
    /// Base currency/asset (e.g., "BTC", "PURR").
    pub base: String,
    /// Quote currency (e.g., "USD" for perps, "USDC" for spot).
    pub quote: String,
    /// Market type (perpetual or spot).
    pub market_type: HyperliquidMarketType,
    /// Number of decimal places for price precision.
    pub price_decimals: u32,
    /// Number of decimal places for size precision.
    pub size_decimals: u32,
    /// Price tick size as decimal.
    pub tick_size: Decimal,
    /// Size lot increment as decimal.
    pub lot_size: Decimal,
    /// Maximum leverage (for perps).
    pub max_leverage: Option<u32>,
    /// Whether requires isolated margin only.
    pub only_isolated: bool,
    /// Whether the instrument is active/tradeable.
    pub active: bool,
    /// Raw upstream data for debugging.
    pub raw_data: String,
}

/// Parse perpetual instrument definitions from Hyperliquid `meta` response.
///
/// Hyperliquid perps follow specific rules:
/// - Quote is always USD (USDC settled)
/// - Price decimals = max(0, 6 - sz_decimals) per venue docs
/// - Active = !is_delisted
///
/// **Important:** Delisted instruments are included in the returned list but marked as inactive.
/// This is necessary to support parsing historical data (orders, fills, positions) for instruments
/// that have been delisted but may still have associated trading history.
pub fn parse_perp_instruments(meta: &PerpMeta) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const PERP_MAX_DECIMALS: i32 = 6; // Hyperliquid perps price decimal limit

    let mut defs = Vec::new();

    for asset in meta.universe.iter() {
        // Include delisted assets but mark them as inactive
        // This allows parsing of historical data for delisted instruments
        let is_delisted = asset.is_delisted.unwrap_or(false);

        let price_decimals = (PERP_MAX_DECIMALS - asset.sz_decimals as i32).max(0) as u32;
        let tick_size = pow10_neg(price_decimals)?;
        let lot_size = pow10_neg(asset.sz_decimals)?;

        let symbol = format!("{}-USD-PERP", asset.name);

        let def = HyperliquidInstrumentDef {
            symbol,
            base: asset.name.clone(),
            quote: "USD".to_string(), // Hyperliquid perps are USD-quoted (USDC settled)
            market_type: HyperliquidMarketType::Perp,
            price_decimals,
            size_decimals: asset.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: asset.max_leverage,
            only_isolated: asset.only_isolated.unwrap_or(false),
            active: !is_delisted, // Mark delisted instruments as inactive
            raw_data: serde_json::to_string(asset).unwrap_or_default(),
        };

        defs.push(def);
    }

    Ok(defs)
}

/// Parse spot instrument definitions from Hyperliquid `spotMeta` response.
///
/// Hyperliquid spot follows these rules:
/// - Price decimals = max(0, 8 - base_sz_decimals) per venue docs
/// - Size decimals from base token
/// - All pairs are loaded (including non-canonical) to support parsing fills/positions
///   for instruments that may have been traded
pub fn parse_spot_instruments(meta: &SpotMeta) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const SPOT_MAX_DECIMALS: i32 = 8; // Hyperliquid spot price decimal limit

    let mut defs = Vec::new();

    // Build index -> token lookup
    let mut tokens_by_index = std::collections::HashMap::new();
    for token in &meta.tokens {
        tokens_by_index.insert(token.index, token);
    }

    for pair in &meta.universe {
        // Load all pairs (including non-canonical) to support parsing fills/positions
        // for instruments that may have been traded but are not currently canonical

        // Resolve base and quote tokens
        let base_token = tokens_by_index
            .get(&pair.tokens[0])
            .ok_or_else(|| format!("Base token index {} not found", pair.tokens[0]))?;
        let quote_token = tokens_by_index
            .get(&pair.tokens[1])
            .ok_or_else(|| format!("Quote token index {} not found", pair.tokens[1]))?;

        let price_decimals = (SPOT_MAX_DECIMALS - base_token.sz_decimals as i32).max(0) as u32;
        let tick_size = pow10_neg(price_decimals)?;
        let lot_size = pow10_neg(base_token.sz_decimals)?;

        let symbol = format!("{}-{}-SPOT", base_token.name, quote_token.name);

        let def = HyperliquidInstrumentDef {
            symbol,
            base: base_token.name.clone(),
            quote: quote_token.name.clone(),
            market_type: HyperliquidMarketType::Spot,
            price_decimals,
            size_decimals: base_token.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: None,
            only_isolated: false,
            active: pair.is_canonical, // Use canonical status to indicate if pair is actively tradeable
            raw_data: serde_json::to_string(pair).unwrap_or_default(),
        };

        defs.push(def);
    }

    Ok(defs)
}

/// Compute 10^(-decimals) as a Decimal.
///
/// This uses integer arithmetic to avoid floating-point precision issues.
fn pow10_neg(decimals: u32) -> Result<Decimal, String> {
    if decimals == 0 {
        return Ok(Decimal::ONE);
    }

    // Build 1 / 10^decimals using integer arithmetic
    Ok(Decimal::from_i128_with_scale(1, decimals))
}

// ================================================================================================
// Instrument Conversion Functions
// ================================================================================================

use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    currencies::CURRENCY_MAP,
    enums::CurrencyType,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};

use crate::common::consts::HYPERLIQUID_VENUE;

fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .expect("Failed to acquire CURRENCY_MAP lock")
        .get(code)
        .copied()
        .unwrap_or_else(|| Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

/// Converts a single Hyperliquid instrument definition into a Nautilus `InstrumentAny`.
///
/// Returns `None` if the conversion fails (e.g., unsupported market type).
#[must_use]
pub fn create_instrument_from_def(def: &HyperliquidInstrumentDef) -> Option<InstrumentAny> {
    let clock = get_atomic_clock_realtime();
    let ts_event = clock.get_time_ns();
    let ts_init = ts_event;

    let symbol = Symbol::new(&def.symbol);
    let venue = *HYPERLIQUID_VENUE;
    let instrument_id = InstrumentId::new(symbol, venue);

    let raw_symbol = Symbol::new(&def.symbol);
    let base_currency = get_currency(&def.base);
    let quote_currency = get_currency(&def.quote);
    let price_increment = Price::from(&def.tick_size.to_string());
    let size_increment = Quantity::from(&def.lot_size.to_string());

    match def.market_type {
        HyperliquidMarketType::Spot => Some(InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            def.price_decimals as u8,
            def.size_decimals as u8,
            price_increment,
            size_increment,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            ts_event,
            ts_init,
        ))),
        HyperliquidMarketType::Perp => {
            let settlement_currency = get_currency("USDC");

            Some(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                false,
                def.price_decimals as u8,
                def.size_decimals as u8,
                price_increment,
                size_increment,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                ts_event,
                ts_init,
            )))
        }
    }
}

/// Convert a collection of Hyperliquid instrument definitions into Nautilus instruments,
/// discarding any definitions that fail to convert.
#[must_use]
pub fn instruments_from_defs(defs: &[HyperliquidInstrumentDef]) -> Vec<InstrumentAny> {
    defs.iter().filter_map(create_instrument_from_def).collect()
}

/// Convert owned definitions into Nautilus instruments, consuming the input vector.
#[must_use]
pub fn instruments_from_defs_owned(defs: Vec<HyperliquidInstrumentDef>) -> Vec<InstrumentAny> {
    defs.into_iter()
        .filter_map(|def| create_instrument_from_def(&def))
        .collect()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::{
        super::models::{PerpAsset, SpotPair, SpotToken},
        *,
    };

    #[rstest]
    fn test_pow10_neg() {
        assert_eq!(pow10_neg(0).unwrap(), Decimal::from(1));
        assert_eq!(pow10_neg(1).unwrap(), Decimal::from_str("0.1").unwrap());
        assert_eq!(pow10_neg(5).unwrap(), Decimal::from_str("0.00001").unwrap());
    }

    #[test]
    fn test_parse_perp_instruments() {
        let meta = PerpMeta {
            universe: vec![
                PerpAsset {
                    name: "BTC".to_string(),
                    sz_decimals: 5,
                    max_leverage: Some(50),
                    only_isolated: None,
                    is_delisted: None,
                },
                PerpAsset {
                    name: "DELIST".to_string(),
                    sz_decimals: 3,
                    max_leverage: Some(10),
                    only_isolated: Some(true),
                    is_delisted: Some(true), // Should be included but marked as inactive
                },
            ],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta).unwrap();

        // Should have both BTC and DELIST (delisted instruments are included for historical data)
        assert_eq!(defs.len(), 2);

        let btc = &defs[0];
        assert_eq!(btc.symbol, "BTC-USD-PERP");
        assert_eq!(btc.base, "BTC");
        assert_eq!(btc.quote, "USD");
        assert_eq!(btc.market_type, HyperliquidMarketType::Perp);
        assert_eq!(btc.price_decimals, 1); // 6 - 5 = 1
        assert_eq!(btc.size_decimals, 5);
        assert_eq!(btc.tick_size, Decimal::from_str("0.1").unwrap());
        assert_eq!(btc.lot_size, Decimal::from_str("0.00001").unwrap());
        assert_eq!(btc.max_leverage, Some(50));
        assert!(!btc.only_isolated);
        assert!(btc.active);

        let delist = &defs[1];
        assert_eq!(delist.symbol, "DELIST-USD-PERP");
        assert_eq!(delist.base, "DELIST");
        assert!(!delist.active); // Delisted instruments are marked as inactive
    }

    #[rstest]
    fn test_parse_spot_instruments() {
        let tokens = vec![
            SpotToken {
                name: "USDC".to_string(),
                sz_decimals: 6,
                wei_decimals: 6,
                index: 0,
                token_id: "0x1".to_string(),
                is_canonical: true,
                evm_contract: None,
                full_name: None,
                deployer_trading_fee_share: None,
            },
            SpotToken {
                name: "PURR".to_string(),
                sz_decimals: 0,
                wei_decimals: 5,
                index: 1,
                token_id: "0x2".to_string(),
                is_canonical: true,
                evm_contract: None,
                full_name: None,
                deployer_trading_fee_share: None,
            },
        ];

        let pairs = vec![
            SpotPair {
                name: "PURR/USDC".to_string(),
                tokens: [1, 0], // PURR base, USDC quote
                index: 0,
                is_canonical: true,
            },
            SpotPair {
                name: "ALIAS".to_string(),
                tokens: [1, 0],
                index: 1,
                is_canonical: false, // Should be included but marked as inactive
            },
        ];

        let meta = SpotMeta {
            tokens,
            universe: pairs,
        };

        let defs = parse_spot_instruments(&meta).unwrap();

        // Should have both PURR/USDC and ALIAS (non-canonical pairs are included for historical data)
        assert_eq!(defs.len(), 2);

        let purr_usdc = &defs[0];
        assert_eq!(purr_usdc.symbol, "PURR-USDC-SPOT");
        assert_eq!(purr_usdc.base, "PURR");
        assert_eq!(purr_usdc.quote, "USDC");
        assert_eq!(purr_usdc.market_type, HyperliquidMarketType::Spot);
        assert_eq!(purr_usdc.price_decimals, 8); // 8 - 0 = 8 (PURR sz_decimals = 0)
        assert_eq!(purr_usdc.size_decimals, 0);
        assert_eq!(
            purr_usdc.tick_size,
            Decimal::from_str("0.00000001").unwrap()
        );
        assert_eq!(purr_usdc.lot_size, Decimal::from(1));
        assert_eq!(purr_usdc.max_leverage, None);
        assert!(!purr_usdc.only_isolated);
        assert!(purr_usdc.active);

        let alias = &defs[1];
        assert_eq!(alias.symbol, "PURR-USDC-SPOT");
        assert_eq!(alias.base, "PURR");
        assert!(!alias.active); // Non-canonical pairs are marked as inactive
    }

    #[rstest]
    fn test_price_decimals_clamping() {
        // Test that price decimals are clamped to >= 0
        let meta = PerpMeta {
            universe: vec![PerpAsset {
                name: "HIGHPREC".to_string(),
                sz_decimals: 10, // 6 - 10 = -4, should clamp to 0
                max_leverage: Some(1),
                only_isolated: None,
                is_delisted: None,
            }],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta).unwrap();
        assert_eq!(defs[0].price_decimals, 0);
        assert_eq!(defs[0].tick_size, Decimal::from(1));
    }
}

////////////////////////////////////////////////////////////////////////////////
// Order, Fill, and Position Report Parsing
////////////////////////////////////////////////////////////////////////////////

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce,
        TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, PositionId, TradeId, VenueOrderId},
    instruments::Instrument,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use rust_decimal::prelude::ToPrimitive;

use super::models::HyperliquidFill;
use crate::{
    common::enums::HyperliquidSide,
    websocket::messages::{WsBasicOrderData, WsOrderData},
};

/// Map Hyperliquid order side to Nautilus OrderSide.
fn parse_order_side(side: &str) -> OrderSide {
    match side.to_lowercase().as_str() {
        "a" | "buy" => OrderSide::Buy,
        "b" | "sell" => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    }
}

/// Map Hyperliquid fill side to Nautilus OrderSide.
fn parse_fill_side(side: &HyperliquidSide) -> OrderSide {
    match side {
        HyperliquidSide::Buy => OrderSide::Buy,
        HyperliquidSide::Sell => OrderSide::Sell,
    }
}

/// Map Hyperliquid order status string to Nautilus OrderStatus.
pub fn parse_order_status(status: &str) -> OrderStatus {
    match status.to_lowercase().as_str() {
        "open" => OrderStatus::Accepted,
        "filled" => OrderStatus::Filled,
        "canceled" | "cancelled" => OrderStatus::Canceled,
        "rejected" => OrderStatus::Rejected,
        "triggered" => OrderStatus::Triggered,
        "partial_fill" | "partially_filled" => OrderStatus::PartiallyFilled,
        _ => OrderStatus::Accepted, // Default to accepted for unknown statuses
    }
}

/// Parse WebSocket order data to OrderStatusReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_order_status_report_from_ws(
    order_data: &WsOrderData,
    instrument: &dyn Instrument,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    parse_order_status_report_from_basic(
        &order_data.order,
        &order_data.status,
        instrument,
        account_id,
        ts_init,
    )
}

/// Parse basic order data to OrderStatusReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_order_status_report_from_basic(
    order: &WsBasicOrderData,
    status_str: &str,
    instrument: &dyn Instrument,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    use nautilus_model::types::{Price, Quantity};
    use rust_decimal::Decimal;

    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.oid.to_string());
    let order_side = parse_order_side(&order.side);

    // Determine order type based on trigger parameters
    let order_type = if order.trigger_px.is_some() {
        if order.is_market == Some(true) {
            // Check if it's stop-loss or take-profit based on tpsl field
            match order.tpsl.as_deref() {
                Some("tp") => OrderType::MarketIfTouched,
                Some("sl") => OrderType::StopMarket,
                _ => OrderType::StopMarket,
            }
        } else {
            match order.tpsl.as_deref() {
                Some("tp") => OrderType::LimitIfTouched,
                Some("sl") => OrderType::StopLimit,
                _ => OrderType::StopLimit,
            }
        }
    } else {
        OrderType::Limit
    };

    let time_in_force = TimeInForce::Gtc; // Hyperliquid uses GTC by default
    let order_status = parse_order_status(status_str);

    // Parse quantities
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let orig_sz: Decimal = order
        .orig_sz
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse orig_sz: {}", e))?;
    let current_sz: Decimal = order
        .sz
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse sz: {}", e))?;

    let quantity = Quantity::new(orig_sz.abs().to_f64().unwrap_or(0.0), size_precision);
    let filled_sz = orig_sz.abs() - current_sz.abs();
    let filled_qty = Quantity::new(filled_sz.to_f64().unwrap_or(0.0), size_precision);

    // Timestamps
    let ts_accepted = UnixNanos::from(order.timestamp * 1_000_000); // Convert ms to ns
    let ts_last = ts_accepted;

    let report_id = UUID4::new();

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id - will be set if present
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        Some(report_id),
    );

    // Add client order ID if present
    if let Some(cloid) = &order.cloid {
        report = report.with_client_order_id(ClientOrderId::new(cloid.as_str()));
    }

    // Add price
    let limit_px: Decimal = order
        .limit_px
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse limit_px: {}", e))?;
    report = report.with_price(Price::new(
        limit_px.to_f64().unwrap_or(0.0),
        price_precision,
    ));

    // Add trigger price if present
    if let Some(trigger_px) = &order.trigger_px {
        let trig_px: Decimal = trigger_px
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse trigger_px: {}", e))?;
        report = report
            .with_trigger_price(Price::new(trig_px.to_f64().unwrap_or(0.0), price_precision))
            .with_trigger_type(TriggerType::Default);
    }

    Ok(report)
}

/// Parse Hyperliquid fill to FillReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_fill_report(
    fill: &HyperliquidFill,
    instrument: &dyn Instrument,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    use nautilus_model::types::{Money, Price, Quantity};
    use rust_decimal::Decimal;

    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(fill.oid.to_string());

    // Construct trade_id from hash and time
    let trade_id_str = format!("{}-{}", fill.hash, fill.time);
    tracing::debug!(
        "Parsing fill: hash={}, time={}, trade_id_str='{}', len={}",
        fill.hash,
        fill.time,
        trade_id_str,
        trade_id_str.len()
    );

    let trade_id = TradeId::new(trade_id_str);
    let order_side = parse_fill_side(&fill.side);

    // Parse price and quantity
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let px: Decimal = fill
        .px
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse fill price: {}", e))?;
    let sz: Decimal = fill
        .sz
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse fill size: {}", e))?;

    let last_px = Price::new(px.to_f64().unwrap_or(0.0), price_precision);
    let last_qty = Quantity::new(sz.abs().to_f64().unwrap_or(0.0), size_precision);

    // Parse fee - Hyperliquid fees are typically in USDC for perps
    let fee_amount: Decimal = fill
        .fee
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse fee: {}", e))?;

    // Determine fee currency - Hyperliquid perp fees are in USDC
    let fee_currency = Currency::from("USDC");
    let commission = Money::new(fee_amount.abs().to_f64().unwrap_or(0.0), fee_currency);

    // Determine liquidity side based on 'crossed' flag
    let liquidity_side = if fill.crossed {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    // Timestamp
    let ts_event = UnixNanos::from(fill.time * 1_000_000); // Convert ms to ns

    let report_id = UUID4::new();

    let report = FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        None, // client_order_id - to be linked by execution engine
        None, // venue_position_id
        ts_event,
        ts_init,
        Some(report_id),
    );

    Ok(report)
}

/// Parse position data from clearinghouse state to PositionStatusReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_position_status_report(
    position_data: &serde_json::Value,
    instrument: &dyn Instrument,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    use nautilus_model::types::Quantity;

    use super::models::AssetPosition;

    // Deserialize the position data
    let asset_position: AssetPosition = serde_json::from_value(position_data.clone())
        .context("Failed to deserialize AssetPosition")?;

    let position = &asset_position.position;
    let instrument_id = instrument.id();

    // Determine position side based on size (szi)
    let (position_side, quantity_value) = if position.szi.is_zero() {
        (PositionSideSpecified::Flat, Decimal::ZERO)
    } else if position.szi.is_sign_positive() {
        (PositionSideSpecified::Long, position.szi)
    } else {
        (PositionSideSpecified::Short, position.szi.abs())
    };

    // Create quantity
    let quantity = Quantity::new(
        quantity_value
            .to_f64()
            .context("Failed to convert quantity to f64")?,
        instrument.size_precision(),
    );

    // Generate report ID
    let report_id = UUID4::new();

    // Use current time as ts_last (could be enhanced with actual last update time if available)
    let ts_last = ts_init;

    // Create position ID from coin symbol
    let venue_position_id = Some(PositionId::new(format!("{}_{}", account_id, position.coin)));

    // Entry price (if available)
    let avg_px_open = position.entry_px;

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        Some(report_id),
        venue_position_id,
        avg_px_open,
    ))
}

#[cfg(test)]
mod reconciliation_tests {
    use super::*;

    #[test]
    fn test_parse_order_side() {
        assert_eq!(parse_order_side("A"), OrderSide::Buy);
        assert_eq!(parse_order_side("buy"), OrderSide::Buy);
        assert_eq!(parse_order_side("B"), OrderSide::Sell);
        assert_eq!(parse_order_side("sell"), OrderSide::Sell);
        assert_eq!(parse_order_side("unknown"), OrderSide::NoOrderSide);
    }

    #[test]
    fn test_parse_order_status() {
        assert_eq!(parse_order_status("open"), OrderStatus::Accepted);
        assert_eq!(parse_order_status("filled"), OrderStatus::Filled);
        assert_eq!(parse_order_status("canceled"), OrderStatus::Canceled);
        assert_eq!(parse_order_status("cancelled"), OrderStatus::Canceled);
        assert_eq!(parse_order_status("rejected"), OrderStatus::Rejected);
        assert_eq!(parse_order_status("triggered"), OrderStatus::Triggered);
    }

    #[test]
    fn test_parse_fill_side() {
        assert_eq!(parse_fill_side(&HyperliquidSide::Buy), OrderSide::Buy);
        assert_eq!(parse_fill_side(&HyperliquidSide::Sell), OrderSide::Sell);
    }

    #[test]
    fn test_parse_order_side_case_insensitive() {
        assert_eq!(parse_order_side("A"), OrderSide::Buy);
        assert_eq!(parse_order_side("a"), OrderSide::Buy);
        assert_eq!(parse_order_side("BUY"), OrderSide::Buy);
        assert_eq!(parse_order_side("Buy"), OrderSide::Buy);
        assert_eq!(parse_order_side("B"), OrderSide::Sell);
        assert_eq!(parse_order_side("b"), OrderSide::Sell);
        assert_eq!(parse_order_side("SELL"), OrderSide::Sell);
        assert_eq!(parse_order_side("Sell"), OrderSide::Sell);
    }

    #[test]
    fn test_parse_order_status_edge_cases() {
        assert_eq!(parse_order_status("OPEN"), OrderStatus::Accepted);
        assert_eq!(parse_order_status("FILLED"), OrderStatus::Filled);
        assert_eq!(parse_order_status(""), OrderStatus::Accepted);
        assert_eq!(parse_order_status("unknown_status"), OrderStatus::Accepted);
    }
}

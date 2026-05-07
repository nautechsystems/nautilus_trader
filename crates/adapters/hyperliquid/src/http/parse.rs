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

use anyhow::Context;
use chrono::{NaiveDateTime, Utc};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        AssetClass, CurrencyType, LiquiditySide, OrderSide, OrderStatus, OrderType,
        PositionSideSpecified, TimeInForce, TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{BinaryOption, CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ustr::Ustr;

use super::models::{
    AssetPosition, HyperliquidFill, OutcomeDescriptor, OutcomeMetaResponse, OutcomeQuestion,
    PerpMeta, SpotBalance, SpotMeta,
};
use crate::{
    common::{
        consts::HYPERLIQUID_VENUE,
        enums::{
            HyperliquidFillDirection, HyperliquidOrderStatus as HyperliquidOrderStatusEnum,
            HyperliquidSide, HyperliquidTpSl,
        },
        parse::make_fill_trade_id,
    },
    websocket::messages::{WsBasicOrderData, WsOrderData},
};

/// Market type enumeration for normalized instrument definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidMarketType {
    /// Perpetual futures contract.
    Perp,
    /// Spot trading pair.
    Spot,
    /// Outcome (prediction) market.
    Outcome,
}

/// Normalized instrument definition produced by this parser.
///
/// This deliberately avoids any tight coupling to Nautilus' Cython types.
/// The InstrumentProvider can later convert this into Nautilus `Instrument`s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidInstrumentDef {
    /// Human-readable symbol (e.g., "BTC-USD-PERP", "PURR-USDC-SPOT").
    pub symbol: Ustr,
    /// Raw symbol used in Hyperliquid WebSocket subscriptions/messages.
    /// For perps: base currency (e.g., "BTC").
    /// For spot: `@{pair_index}` format (e.g., "@107" for HYPE-USDC).
    /// For outcomes: `#{asset}` format (e.g., "#20").
    pub raw_symbol: Ustr,
    /// Base currency/asset (e.g., "BTC", "PURR").
    pub base: Ustr,
    /// Quote currency (e.g., "USD" for perps, "USDC" for spot).
    pub quote: Ustr,
    /// Market type (perpetual or spot).
    pub market_type: HyperliquidMarketType,
    /// Asset index used for order submission.
    /// For perps: index in meta.universe (0, 1, 2, ...).
    /// For spot: 10000 + index in spotMeta.universe.
    pub asset_index: u32,
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
    /// Whether this is a HIP-3 builder-deployed perpetual.
    pub is_hip3: bool,
    /// Whether the instrument is active/tradeable.
    pub active: bool,
    /// Raw upstream data for debugging.
    pub raw_data: String,
}

// Replace wildcard bytes (`*`, `?`) in a venue-supplied symbol component with
// `x` so the value is safe to embed in a Nautilus `InstrumentId`. HIP-3
// perpetual names from Hyperliquid (e.g. `dex:STREAMABCD****-USD-PERP`)
// collide with msgbus pattern syntax; the venue-official name is preserved on
// `raw_symbol` for HTTP/WS wire calls, and orders use the numeric
// `asset_index` so they do not see the substitution.
#[must_use]
fn sanitize_symbol(value: &str) -> std::borrow::Cow<'_, str> {
    if value.bytes().any(|b| b == b'*' || b == b'?') {
        let mut out = String::with_capacity(value.len());
        for ch in value.chars() {
            out.push(if ch == '*' || ch == '?' { 'x' } else { ch });
        }
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(value)
    }
}

/// Parse perpetual instrument definitions from Hyperliquid `meta` response.
///
/// Hyperliquid perps follow specific rules:
/// - Quote is always USD (USDC settled)
/// - Price decimals = max(0, 6 - sz_decimals) per venue docs
/// - Active = !is_delisted
///
/// `asset_index_base` controls the starting offset for asset IDs:
/// - Standard perps (dex 0): base = 0
/// - HIP-3 dexes: base = 100_000 + dex_index * 10_000
///
/// Delisted instruments are included but marked as inactive to support
/// parsing historical data for instruments that may still have trading history.
pub fn parse_perp_instruments(
    meta: &PerpMeta,
    asset_index_base: u32,
) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const PERP_MAX_DECIMALS: i32 = 6;

    let mut defs = Vec::new();

    for (index, asset) in meta.universe.iter().enumerate() {
        let is_delisted = asset.is_delisted.unwrap_or(false);

        let price_decimals = (PERP_MAX_DECIMALS - asset.sz_decimals as i32).max(0) as u32;
        let tick_size = pow10_neg(price_decimals);
        let lot_size = pow10_neg(asset.sz_decimals);

        let symbol = format!("{}-USD-PERP", sanitize_symbol(&asset.name));

        let raw_symbol: Ustr = asset.name.as_str().into();

        let def = HyperliquidInstrumentDef {
            symbol: symbol.into(),
            raw_symbol,
            base: asset.name.clone().into(),
            quote: "USD".into(),
            market_type: HyperliquidMarketType::Perp,
            asset_index: asset_index_base + index as u32,
            price_decimals,
            size_decimals: asset.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: asset.max_leverage,
            only_isolated: asset.only_isolated.unwrap_or(false),
            is_hip3: asset_index_base > 0,
            active: !is_delisted,
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
    const SPOT_INDEX_OFFSET: u32 = 10000; // Spot assets use 10000 + index

    let mut defs = Vec::new();

    // Build index -> token lookup
    let mut tokens_by_index = ahash::AHashMap::new();
    for token in &meta.tokens {
        tokens_by_index.insert(token.index, token);
    }

    for pair in &meta.universe {
        // Load all pairs (including non-canonical) to support parsing fills/positions
        // for instruments that may have been traded but are not currently canonical

        let base_token = tokens_by_index
            .get(&pair.tokens[0])
            .ok_or_else(|| format!("Base token index {} not found", pair.tokens[0]))?;
        let quote_token = tokens_by_index
            .get(&pair.tokens[1])
            .ok_or_else(|| format!("Quote token index {} not found", pair.tokens[1]))?;

        let price_decimals = (SPOT_MAX_DECIMALS - base_token.sz_decimals as i32).max(0) as u32;
        let tick_size = pow10_neg(price_decimals);
        let lot_size = pow10_neg(base_token.sz_decimals);

        let symbol = format!(
            "{}-{}-SPOT",
            sanitize_symbol(&base_token.name),
            sanitize_symbol(&quote_token.name),
        );

        // Hyperliquid spot raw_symbol formats (per API docs):
        // - PURR uses slash format from pair.name (e.g., "PURR/USDC")
        // - All others use "@{pair_index}" format (e.g., "@107" for HYPE)
        let raw_symbol: Ustr = if base_token.name == "PURR" {
            pair.name.as_str().into()
        } else {
            format!("@{}", pair.index).into()
        };

        let def = HyperliquidInstrumentDef {
            symbol: symbol.into(),
            raw_symbol,
            base: base_token.name.clone().into(),
            quote: quote_token.name.clone().into(),
            market_type: HyperliquidMarketType::Spot,
            asset_index: SPOT_INDEX_OFFSET + pair.index,
            price_decimals,
            size_decimals: base_token.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: None,
            only_isolated: false,
            is_hip3: false,
            active: pair.is_canonical, // Use canonical status to indicate if pair is actively tradeable
            raw_data: serde_json::to_string(pair).unwrap_or_default(),
        };

        defs.push(def);
    }

    // Canonical pairs must be cached first so the base-token alias (e.g.
    // "PURR" -> PURR-USDC-SPOT) resolves to the canonical instrument when
    // non-canonical pairs share the same base. Secondary key keeps the
    // order stable within each bucket.
    defs.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(a.asset_index.cmp(&b.asset_index))
    });

    Ok(defs)
}

/// Parse outcome (prediction) market instrument definitions from Hyperliquid `outcomeMeta` response.
///
/// Each outcome market generates two instrument definitions (Yes/No sides).
/// Data-coin encoding: `asset = outcome_id * 10 + side`
/// where side is 0 for first side (Yes), 1 for second side (No).
///
/// Hyperliquid action asset IDs for outcomes are offset by `100_000_000`:
/// `action_asset = 100_000_000 + asset`.
pub fn parse_outcome_instruments(
    meta: &OutcomeMetaResponse,
) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const OUTCOME_ACTION_ASSET_OFFSET: u32 = 100_000_000;
    // Outcome markets use 6 decimal places for price precision (0.000001 increments)
    const OUTCOME_PRICE_DECIMALS: u32 = 6;
    const OUTCOME_SIZE_DECIMALS: u32 = 6; // USDH precision

    // Build question -> named outcome mapping for categorical questions (e.g., priceBucket with 3
    // buckets: Down / Range / Up).
    let mut question_by_outcome_id: ahash::AHashMap<u32, (OutcomeQuestion, u8)> =
        ahash::AHashMap::new();

    for q in &meta.questions {
        if let Some(fallback_outcome) = q.fallback_outcome {
            question_by_outcome_id.insert(fallback_outcome, (q.clone(), u8::MAX));
        }

        for (idx, outcome_id) in q.named_outcomes.iter().enumerate() {
            let Ok(idx_u8) = u8::try_from(idx) else {
                continue;
            };
            question_by_outcome_id.insert(*outcome_id, (q.clone(), idx_u8));
        }
    }

    let mut defs = Vec::new();

    for outcome in &meta.outcomes {
        for (side_idx, side_spec) in outcome.side_specs.iter().enumerate() {
            let side = side_idx as u8;
            let data_asset = outcome.outcome * 10 + side as u32;
            let action_asset = OUTCOME_ACTION_ASSET_OFFSET + data_asset;

            // Symbol format: OUTCOME-{outcome_id}-{YES|NO}-OUTCOME
            let symbol = format!(
                "OUTCOME-{}-{}-OUTCOME",
                outcome.outcome,
                side_spec.name.to_uppercase()
            );

            // Raw symbol for WebSocket/API: "#<asset_index>"
            let raw_symbol = format!("#{data_asset}");

            let tick_size = pow10_neg(OUTCOME_PRICE_DECIMALS);
            let lot_size = pow10_neg(OUTCOME_SIZE_DECIMALS);

            let raw_data = if let Some((question, bucket_index)) =
                question_by_outcome_id.get(&outcome.outcome)
            {
                serde_json::to_string(&serde_json::json!({
                    "outcome": outcome,
                    "question": question,
                    "bucket_index": bucket_index,
                }))
                .unwrap_or_default()
            } else {
                serde_json::to_string(&serde_json::json!({
                    "outcome": outcome,
                }))
                .unwrap_or_default()
            };

            let def = HyperliquidInstrumentDef {
                symbol: symbol.into(),
                raw_symbol: raw_symbol.into(),
                base: format!("{}-{}", outcome.name, side_spec.name).into(),
                quote: "USDH".into(),
                market_type: HyperliquidMarketType::Outcome,
                // Action asset ID used for order submission/cancel paths.
                asset_index: action_asset,
                price_decimals: OUTCOME_PRICE_DECIMALS,
                size_decimals: OUTCOME_SIZE_DECIMALS,
                tick_size,
                lot_size,
                max_leverage: Some(1), // No leverage for prediction markets
                only_isolated: false,
                is_hip3: false,
                active: true,
                raw_data,
            };

            defs.push(def);
        }
    }

    Ok(defs)
}

fn pow10_neg(decimals: u32) -> Decimal {
    if decimals == 0 {
        return Decimal::ONE;
    }

    // Build 1 / 10^decimals using integer arithmetic
    Decimal::from_i128_with_scale(1, decimals)
}

pub fn get_currency(code: &str) -> Currency {
    Currency::try_from_str(code).unwrap_or_else(|| {
        let currency = Currency::new(code, 8, 0, code, CurrencyType::Crypto);
        if let Err(e) = Currency::register(currency, false) {
            log::error!("Failed to register currency '{code}': {e}");
        }
        currency
    })
}

/// Converts a single Hyperliquid instrument definition into a Nautilus `InstrumentAny`.
///
/// Returns `None` if the conversion fails (e.g., unsupported market type).
#[must_use]
pub fn create_instrument_from_def(
    def: &HyperliquidInstrumentDef,
    ts_init: UnixNanos,
) -> Option<InstrumentAny> {
    let symbol = Symbol::new(def.symbol);
    let venue = *HYPERLIQUID_VENUE;
    let instrument_id = InstrumentId::new(symbol, venue);

    // Use the raw_symbol from the definition which is format-specific:
    // - Perps: base currency (e.g., "BTC")
    // - Spot PURR: slash format (e.g., "PURR/USDC")
    // - Spot others: @{index} format (e.g., "@107")
    // - Outcomes: #{asset} format (e.g., "#20")
    let raw_symbol = Symbol::new(def.raw_symbol);
    let price_increment = Price::from(def.tick_size.to_string());
    let size_increment = Quantity::from(def.lot_size.to_string());

    match def.market_type {
        HyperliquidMarketType::Spot => {
            let base_currency = get_currency(&def.base);
            let quote_currency = get_currency(&def.quote);

            Some(InstrumentAny::CurrencyPair(CurrencyPair::new(
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
                None,
                ts_init, // Identical to ts_init for now
                ts_init,
            )))
        }
        HyperliquidMarketType::Perp => {
            let base_currency = get_currency(&def.base);
            let quote_currency = get_currency(&def.quote);
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
                None, // multiplier
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
                ts_init, // Identical to ts_init for now
                ts_init,
            )))
        }
        HyperliquidMarketType::Outcome => {
            // Outcome markets use USDH for settlement
            let currency = get_currency("USDH");

            // Parse raw_data to extract outcome metadata.
            //
            // For categorical questions (e.g., priceBucket Down/Range/Up), `raw_data` includes the
            // parent `question` payload + `bucket_index` so we can derive expiry/thresholds.
            let raw_val: serde_json::Value = serde_json::from_str(&def.raw_data).ok()?;
            let outcome_desc: OutcomeDescriptor =
                serde_json::from_value(raw_val.get("outcome")?.clone()).ok()?;
            let question: Option<OutcomeQuestion> = raw_val
                .get("question")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let bucket_index: Option<u8> = raw_val
                .get("bucket_index")
                .and_then(|v| v.as_u64())
                .and_then(|v| u8::try_from(v).ok());

            let raw_description = outcome_desc.description.as_str();

            let parsed_desc = parse_outcome_description(raw_description);

            let parsed_question_desc = question
                .as_ref()
                .and_then(|q| parse_outcome_question_description(&q.description, bucket_index));

            let (activation_ns, expiration_ns) = match (
                parsed_desc.activation_ns,
                parsed_desc.expiration_ns,
                parsed_question_desc.as_ref().and_then(|p| p.activation_ns),
                parsed_question_desc.as_ref().and_then(|p| p.expiration_ns),
            ) {
                (Some(a), Some(e), _, _) => (a, e),
                (None, Some(e), _, _) => (UnixNanos::default(), e),
                (_, _, Some(a), Some(e)) => (a, e),
                (_, _, None, Some(e)) => (UnixNanos::default(), e),
                _ => (UnixNanos::default(), ts_init),
            };

            let outcome_side = parse_outcome_side_from_symbol(def.symbol.as_str())
                .map(Ustr::from)
                .or_else(|| {
                    outcome_desc
                        .side_specs
                        .first()
                        .map(|s| Ustr::from(s.name.as_str()))
                });

            let description_str = parsed_desc
                .question
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    parsed_question_desc
                        .as_ref()
                        .and_then(|p| p.question.as_deref())
                        .filter(|s| !s.is_empty())
                })
                .or_else(|| (!raw_description.is_empty()).then_some(raw_description))
                .map(str::to_string);

            let description = description_str.as_deref().map(Ustr::from);

            let info: Option<Params> = serde_json::from_value(json!({
	                "hyperliquid": {
	                    "market_type": "outcome",
	                    "symbol": def.symbol.as_str(),
	                    "raw_symbol": def.raw_symbol.as_str(),
	                    "asset_index": def.asset_index,
	                    "outcome": outcome_desc,
	                    "description_raw": raw_description,
	                    // Parsed `priceBinary` parameters (when available). For recurring markets,
	                    // `target_price` is the threshold used for settlement comparisons.
	                    "price_binary": {
	                        "underlying": parsed_desc.underlying,
	                        "period": parsed_desc.period,
	                        "expiry": parsed_desc.expiry,
	                        "target_price": parsed_desc.target_price,
	                    },
	                    // Parsed `priceBucket` parameters (when available). For recurring markets,
	                    // `price_thresholds` defines the two boundaries which create three buckets.
	                    "price_bucket": parsed_question_desc.as_ref().and_then(|p| p.price_bucket.clone()),
	                    "bucket_index": bucket_index,
	                    "question": question,
	                    "description_parsed": parsed_desc.fields,
	                }
	            }))
	            .ok();

            // For outcome markets, we use BinaryOption instrument
            let binary_option = BinaryOption::new_checked(
                instrument_id,
                raw_symbol,
                AssetClass::Alternative, // Prediction markets are alternative assets
                currency,
                activation_ns,
                expiration_ns,
                def.price_decimals as u8,
                def.size_decimals as u8,
                price_increment,
                size_increment,
                outcome_side,
                description,
                None,                       // max_quantity
                None,                       // min_quantity
                None,                       // max_notional
                None,                       // min_notional
                Some(Price::from("0.999")), // max_price
                Some(Price::from("0.001")), // min_price
                None,                       // margin_init - will use default
                None,                       // margin_maint - will use default
                None,                       // maker_fee
                None,                       // taker_fee
                info,
                ts_init,
                ts_init,
            )
            .ok()?;

            Some(InstrumentAny::BinaryOption(binary_option))
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ParsedOutcomeDescription {
    activation_ns: Option<UnixNanos>,
    expiration_ns: Option<UnixNanos>,
    question: Option<String>,
    underlying: Option<String>,
    period: Option<String>,
    target_price: Option<String>,
    expiry: Option<String>,
    fields: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
struct ParsedOutcomeQuestionDescription {
    activation_ns: Option<UnixNanos>,
    expiration_ns: Option<UnixNanos>,
    question: Option<String>,
    price_bucket: Option<serde_json::Value>,
}

fn parse_outcome_description(description: &str) -> ParsedOutcomeDescription {
    // Expected encoding:
    // "class:priceBinary|underlying:BTC|expiry:20260507-0600|targetPrice:81287|period:1d"
    let mut map = serde_json::Map::new();

    for part in description.split('|') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        map.insert(k.to_string(), json!(v));
    }

    let underlying_str = map
        .get("underlying")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let period = map
        .get("period")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let target_price_str = map
        .get("targetPrice")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let expiry_str = map
        .get("expiry")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let expiry = map
        .get("expiry")
        .and_then(|v| v.as_str())
        .and_then(parse_outcome_expiry_to_nanos);

    let period_nanos = map
        .get("period")
        .and_then(|v| v.as_str())
        .and_then(parse_outcome_period_to_nanos);

    let activation_ns = match (expiry, period_nanos) {
        (Some(expiry_ns), Some(delta)) => expiry_ns.as_u64().checked_sub(delta).map(UnixNanos::new),
        _ => None,
    };

    let (underlying_ref, target_price_ref, expiry_iso) = (
        map.get("underlying").and_then(|v| v.as_str()),
        map.get("targetPrice").and_then(|v| v.as_str()),
        expiry.and_then(unix_nanos_to_iso),
    );

    let question = match (
        map.get("class").and_then(|v| v.as_str()),
        underlying_ref,
        target_price_ref,
        expiry_iso.as_deref(),
    ) {
        (Some("priceBinary"), Some(u), Some(tp), Some(exp)) => {
            Some(format!("Will {u} be above {tp} at {exp}?"))
        }
        _ if !description.is_empty() => Some(description.to_string()),
        _ => None,
    };

    ParsedOutcomeDescription {
        activation_ns,
        expiration_ns: expiry,
        question,
        underlying: underlying_str,
        period,
        target_price: target_price_str,
        expiry: expiry_str,
        fields: serde_json::Value::Object(map),
    }
}

fn parse_outcome_question_description(
    description: &str,
    bucket_index: Option<u8>,
) -> Option<ParsedOutcomeQuestionDescription> {
    if description.is_empty() {
        return None;
    }

    let mut map = serde_json::Map::new();

    for part in description.split('|') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        map.insert(k.to_string(), json!(v));
    }

    let class = map.get("class").and_then(|v| v.as_str())?;
    if class != "priceBucket" {
        return None;
    }

    let expiry = map
        .get("expiry")
        .and_then(|v| v.as_str())
        .and_then(parse_outcome_expiry_to_nanos);

    let period_nanos = map
        .get("period")
        .and_then(|v| v.as_str())
        .and_then(parse_outcome_period_to_nanos);

    let activation_ns = match (expiry, period_nanos) {
        (Some(expiry_ns), Some(delta)) => expiry_ns.as_u64().checked_sub(delta).map(UnixNanos::new),
        _ => None,
    };

    let thresholds: Option<Vec<&str>> =
        map.get("priceThresholds")
            .and_then(|v| v.as_str())
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .collect()
            });

    let price_bucket = thresholds.as_ref().and_then(|t| {
        if t.len() != 2 {
            return None;
        }
        Some(json!({
            "class": "priceBucket",
            "underlying": map.get("underlying").and_then(|v| v.as_str()),
            "period": map.get("period").and_then(|v| v.as_str()),
            "expiry": map.get("expiry").and_then(|v| v.as_str()),
            "price_thresholds": [t[0], t[1]],
        }))
    });

    let question = match (
        map.get("underlying").and_then(|v| v.as_str()),
        thresholds.as_ref(),
        expiry.and_then(unix_nanos_to_iso),
        bucket_index,
    ) {
        (Some(u), Some(t), Some(exp), Some(0)) if t.len() == 2 => {
            Some(format!("Will {u} be below {} at {exp}?", t[0]))
        }
        (Some(u), Some(t), Some(exp), Some(1)) if t.len() == 2 => Some(format!(
            "Will {u} be between {} and {} at {exp}?",
            t[0], t[1]
        )),
        (Some(u), Some(t), Some(exp), Some(2)) if t.len() == 2 => {
            Some(format!("Will {u} be above {} at {exp}?", t[1]))
        }
        _ => None,
    };

    Some(ParsedOutcomeQuestionDescription {
        activation_ns,
        expiration_ns: expiry,
        question,
        price_bucket,
    })
}

fn parse_outcome_expiry_to_nanos(expiry: &str) -> Option<UnixNanos> {
    // "YYYYMMDD-HHMM" (UTC)
    let naive = NaiveDateTime::parse_from_str(expiry, "%Y%m%d-%H%M").ok()?;
    let dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
    let secs = dt.timestamp();
    if secs < 0 {
        return None;
    }
    let secs_u64 = secs as u64;
    let nanos = secs_u64
        .checked_mul(1_000_000_000)?
        .checked_add(dt.timestamp_subsec_nanos() as u64)?;
    Some(UnixNanos::new(nanos))
}

fn parse_outcome_period_to_nanos(period: &str) -> Option<u64> {
    if period.len() < 2 {
        return None;
    }
    let (n_str, unit) = period.split_at(period.len() - 1);
    let n: i64 = n_str.parse().ok()?;

    let seconds: i64 = match unit {
        "d" => n.saturating_mul(86_400),
        "h" => n.saturating_mul(3_600),
        "m" => n.saturating_mul(60),
        "s" => n,
        _ => return None,
    };

    if seconds < 0 {
        return None;
    }
    Some((seconds as u64).saturating_mul(1_000_000_000))
}

fn unix_nanos_to_iso(ns: UnixNanos) -> Option<String> {
    let v = ns.as_u64();
    let secs = (v / 1_000_000_000) as i64;
    let nanos = (v % 1_000_000_000) as u32;
    let dt = chrono::DateTime::<Utc>::from_timestamp(secs, nanos)?;
    Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

fn parse_outcome_side_from_symbol(symbol: &str) -> Option<&str> {
    // "OUTCOME-{id}-{YES|NO}-OUTCOME"
    let mut parts = symbol.split('-');
    let _prefix = parts.next()?;
    let _id = parts.next()?;
    let side = parts.next()?;
    Some(side)
}

/// Convert a collection of Hyperliquid instrument definitions into Nautilus instruments,
/// discarding any definitions that fail to convert.
#[must_use]
pub fn instruments_from_defs(
    defs: &[HyperliquidInstrumentDef],
    ts_init: UnixNanos,
) -> Vec<InstrumentAny> {
    defs.iter()
        .filter_map(|def| create_instrument_from_def(def, ts_init))
        .collect()
}

/// Convert owned definitions into Nautilus instruments, consuming the input vector.
#[must_use]
pub fn instruments_from_defs_owned(
    defs: Vec<HyperliquidInstrumentDef>,
    ts_init: UnixNanos,
) -> Vec<InstrumentAny> {
    defs.into_iter()
        .filter_map(|def| create_instrument_from_def(&def, ts_init))
        .collect()
}

fn parse_fill_side(side: &HyperliquidSide) -> OrderSide {
    match side {
        HyperliquidSide::Buy => OrderSide::Buy,
        HyperliquidSide::Sell => OrderSide::Sell,
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
    status: &HyperliquidOrderStatusEnum,
    instrument: &dyn Instrument,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.oid.to_string());
    let order_side = OrderSide::from(order.side);

    // Determine order type based on trigger parameters
    let order_type = if order.trigger_px.is_some() {
        if order.is_market == Some(true) {
            // Check if it's stop-loss or take-profit based on tpsl field
            match order.tpsl.as_ref() {
                Some(HyperliquidTpSl::Tp) => OrderType::MarketIfTouched,
                Some(HyperliquidTpSl::Sl) => OrderType::StopMarket,
                _ => OrderType::StopMarket,
            }
        } else {
            match order.tpsl.as_ref() {
                Some(HyperliquidTpSl::Tp) => OrderType::LimitIfTouched,
                Some(HyperliquidTpSl::Sl) => OrderType::StopLimit,
                _ => OrderType::StopLimit,
            }
        }
    } else {
        OrderType::Limit
    };

    let time_in_force = TimeInForce::Gtc;
    let order_status = OrderStatus::from(*status);

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let orig_sz: Decimal = order
        .orig_sz
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse orig_sz: {e}"))?;
    let current_sz: Decimal = order
        .sz
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse sz: {e}"))?;

    let quantity = Quantity::from_decimal_dp(orig_sz.abs(), size_precision)
        .map_err(|e| anyhow::anyhow!("Failed to create quantity from orig_sz: {e}"))?;
    let filled_sz = orig_sz.abs() - current_sz.abs();
    let filled_qty = Quantity::from_decimal_dp(filled_sz, size_precision)
        .map_err(|e| anyhow::anyhow!("Failed to create quantity from filled_sz: {e}"))?;

    let ts_accepted = UnixNanos::from(order.timestamp * 1_000_000);
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

    // Only set price for non-filled orders. For filled orders, the limit price is not
    // the execution price, and setting it would cause bogus inferred fills to be created
    // during reconciliation. Real fills arrive via the userEvents WebSocket channel.
    if !matches!(
        order_status,
        OrderStatus::Filled | OrderStatus::PartiallyFilled
    ) {
        let limit_px: Decimal = order
            .limit_px
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse limit_px: {e}"))?;
        let price = Price::from_decimal_dp(limit_px, price_precision)
            .map_err(|e| anyhow::anyhow!("Failed to create price from limit_px: {e}"))?;
        report = report.with_price(price);
    }

    // Add trigger price if present
    if let Some(trigger_px) = &order.trigger_px {
        let trig_px: Decimal = trigger_px
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse trigger_px: {e}"))?;
        let trigger_price = Price::from_decimal_dp(trig_px, price_precision)
            .map_err(|e| anyhow::anyhow!("Failed to create trigger price: {e}"))?;
        report = report
            .with_trigger_price(trigger_price)
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
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(fill.oid.to_string());

    if matches!(fill.dir, HyperliquidFillDirection::AutoDeleveraging) {
        log::warn!(
            "Auto-deleveraging fill: {instrument_id} oid={} px={} sz={}",
            fill.oid,
            fill.px,
            fill.sz,
        );
    }

    let trade_id = make_fill_trade_id(
        &fill.hash,
        fill.oid,
        &fill.px,
        &fill.sz,
        fill.time,
        &fill.start_position,
    );
    let order_side = parse_fill_side(&fill.side);

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let px: Decimal = fill
        .px
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse fill price: {e}"))?;
    let sz: Decimal = fill
        .sz
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse fill size: {e}"))?;

    let last_px = Price::from_decimal_dp(px, price_precision)
        .map_err(|e| anyhow::anyhow!("Failed to create price from fill px: {e}"))?;
    let last_qty = Quantity::from_decimal_dp(sz.abs(), size_precision)
        .map_err(|e| anyhow::anyhow!("Failed to create quantity from fill sz: {e}"))?;

    let fee_amount: Decimal = fill
        .fee
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse fee: {e}"))?;

    let fee_currency: Currency = fill
        .fee_token
        .parse()
        .map_err(|e| anyhow::anyhow!("Unknown fee token '{}': {e}", fill.fee_token))?;
    let commission = Money::from_decimal(fee_amount, fee_currency)
        .map_err(|e| anyhow::anyhow!("Failed to create commission from fee: {e}"))?;

    // Determine liquidity side based on 'crossed' flag
    let liquidity_side = if fill.crossed {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let ts_event = UnixNanos::from(fill.time * 1_000_000);
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
    // Deserialize the position data
    let asset_position: AssetPosition = serde_json::from_value(position_data.clone())
        .context("failed to deserialize AssetPosition")?;

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

    let quantity = Quantity::from_decimal_dp(quantity_value, instrument.size_precision())
        .context("failed to create quantity from decimal")?;
    let report_id = UUID4::new();
    let ts_last = ts_init;
    let avg_px_open = position.entry_px;

    // Hyperliquid uses netting (one position per instrument), not hedging
    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        Some(report_id),
        None, // No venue_position_id for netting positions
        avg_px_open,
    ))
}

/// Parse a spot token balance into a [`PositionStatusReport`] against the spot instrument.
///
/// Spot holdings are always Long (Hyperliquid spot has no short exposure). The average
/// entry price is derived from `entry_ntl / total` when both are non-zero; otherwise it
/// is omitted.
///
/// # Errors
///
/// Returns an error if the quantity cannot be constructed at the instrument's precision.
pub fn parse_spot_position_status_report(
    balance: &SpotBalance,
    instrument: &dyn Instrument,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let (position_side, quantity_value) = if balance.total.is_zero() {
        (PositionSideSpecified::Flat, Decimal::ZERO)
    } else {
        (PositionSideSpecified::Long, balance.total)
    };

    let quantity = Quantity::from_decimal_dp(quantity_value, instrument.size_precision())
        .context("failed to create spot quantity from decimal")?;

    Ok(PositionStatusReport::new(
        account_id,
        instrument.id(),
        position_side,
        quantity,
        ts_init,
        ts_init,
        Some(UUID4::new()),
        None,
        balance.avg_entry_px(),
    ))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::{
        super::models::{HyperliquidL2Book, PerpAsset, SpotPair, SpotToken},
        *,
    };

    #[rstest]
    fn test_parse_fill_side() {
        assert_eq!(parse_fill_side(&HyperliquidSide::Buy), OrderSide::Buy);
        assert_eq!(parse_fill_side(&HyperliquidSide::Sell), OrderSide::Sell);
    }

    #[rstest]
    fn test_pow10_neg() {
        assert_eq!(pow10_neg(0), dec!(1));
        assert_eq!(pow10_neg(1), dec!(0.1));
        assert_eq!(pow10_neg(5), dec!(0.00001));
    }

    #[rstest]
    fn test_parse_perp_instruments() {
        let meta = PerpMeta {
            universe: vec![
                PerpAsset {
                    name: "BTC".to_string(),
                    sz_decimals: 5,
                    max_leverage: Some(50),
                    ..Default::default()
                },
                PerpAsset {
                    name: "DELIST".to_string(),
                    sz_decimals: 3,
                    max_leverage: Some(10),
                    only_isolated: Some(true),
                    is_delisted: Some(true),
                    ..Default::default()
                },
            ],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta, 0).unwrap();

        // Should have both BTC and DELIST (delisted instruments are included for historical data)
        assert_eq!(defs.len(), 2);

        let btc = &defs[0];
        assert_eq!(btc.symbol, "BTC-USD-PERP");
        assert_eq!(btc.base, "BTC");
        assert_eq!(btc.quote, "USD");
        assert_eq!(btc.market_type, HyperliquidMarketType::Perp);
        assert_eq!(btc.price_decimals, 1); // 6 - 5 = 1
        assert_eq!(btc.size_decimals, 5);
        assert_eq!(btc.tick_size, dec!(0.1));
        assert_eq!(btc.lot_size, dec!(0.00001));
        assert_eq!(btc.max_leverage, Some(50));
        assert!(!btc.only_isolated);
        assert!(btc.active);

        let delist = &defs[1];
        assert_eq!(delist.symbol, "DELIST-USD-PERP");
        assert_eq!(delist.base, "DELIST");
        assert!(!delist.active); // Delisted instruments are marked as inactive
    }

    use crate::common::testing::load_test_data;

    #[rstest]
    fn test_parse_perp_instruments_from_real_data() {
        let meta: PerpMeta = load_test_data("http_meta_perp_sample.json");

        let defs = parse_perp_instruments(&meta, 0).unwrap();

        // Should have 3 instruments (BTC, ETH, ATOM)
        assert_eq!(defs.len(), 3);

        // Validate BTC
        let btc = &defs[0];
        assert_eq!(btc.symbol, "BTC-USD-PERP");
        assert_eq!(btc.base, "BTC");
        assert_eq!(btc.quote, "USD");
        assert_eq!(btc.market_type, HyperliquidMarketType::Perp);
        assert_eq!(btc.size_decimals, 5);
        assert_eq!(btc.max_leverage, Some(40));
        assert!(btc.active);

        // Validate ETH
        let eth = &defs[1];
        assert_eq!(eth.symbol, "ETH-USD-PERP");
        assert_eq!(eth.base, "ETH");
        assert_eq!(eth.size_decimals, 4);
        assert_eq!(eth.max_leverage, Some(25));

        // Validate ATOM
        let atom = &defs[2];
        assert_eq!(atom.symbol, "ATOM-USD-PERP");
        assert_eq!(atom.base, "ATOM");
        assert_eq!(atom.size_decimals, 2);
        assert_eq!(atom.max_leverage, Some(5));
    }

    #[rstest]
    fn test_deserialize_l2_book_from_real_data() {
        let book: HyperliquidL2Book = load_test_data("http_l2_book_btc.json");

        // Validate basic structure
        assert_eq!(book.coin, "BTC");
        assert_eq!(book.levels.len(), 2); // [bids, asks]
        assert_eq!(book.levels[0].len(), 5); // 5 bid levels
        assert_eq!(book.levels[1].len(), 5); // 5 ask levels

        // Verify bids and asks are properly ordered
        let bids = &book.levels[0];
        let asks = &book.levels[1];

        // Bids should be descending (highest first)
        for i in 1..bids.len() {
            let prev_price = bids[i - 1].px.parse::<f64>().unwrap();
            let curr_price = bids[i].px.parse::<f64>().unwrap();
            assert!(prev_price >= curr_price, "Bids should be descending");
        }

        // Asks should be ascending (lowest first)
        for i in 1..asks.len() {
            let prev_price = asks[i - 1].px.parse::<f64>().unwrap();
            let curr_price = asks[i].px.parse::<f64>().unwrap();
            assert!(prev_price <= curr_price, "Asks should be ascending");
        }
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
        assert_eq!(purr_usdc.tick_size, dec!(0.00000001));
        assert_eq!(purr_usdc.lot_size, dec!(1));
        assert_eq!(purr_usdc.max_leverage, None);
        assert!(!purr_usdc.only_isolated);
        assert!(purr_usdc.active);

        let alias = &defs[1];
        assert_eq!(alias.symbol, "PURR-USDC-SPOT");
        assert_eq!(alias.base, "PURR");
        assert!(!alias.active); // Non-canonical pairs are marked as inactive
    }

    #[rstest]
    fn test_parse_spot_instruments_sorts_canonical_before_non_canonical() {
        // Non-canonical pair uses a lower pair index than the canonical one;
        // the sort must still put canonical first so the base-token alias in
        // cache_instrument resolves to the canonical instrument.
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
                name: "HYPE".to_string(),
                sz_decimals: 2,
                wei_decimals: 8,
                index: 150,
                token_id: "0x2".to_string(),
                is_canonical: true,
                evm_contract: None,
                full_name: None,
                deployer_trading_fee_share: None,
            },
        ];

        let pairs = vec![
            SpotPair {
                name: "HYPE_OLD".to_string(),
                tokens: [150, 0],
                index: 3,
                is_canonical: false,
            },
            SpotPair {
                name: "HYPE".to_string(),
                tokens: [150, 0],
                index: 107,
                is_canonical: true,
            },
        ];

        let defs = parse_spot_instruments(&SpotMeta {
            tokens,
            universe: pairs,
        })
        .unwrap();

        assert_eq!(defs.len(), 2);
        assert!(defs[0].active, "canonical must sort first");
        assert_eq!(defs[0].asset_index, 10000 + 107);
        assert!(!defs[1].active);
        assert_eq!(defs[1].asset_index, 10000 + 3);
    }

    #[rstest]
    fn test_price_decimals_clamping() {
        let meta = PerpMeta {
            universe: vec![PerpAsset {
                name: "HIGHPREC".to_string(),
                sz_decimals: 10, // 6 - 10 = -4, should clamp to 0
                max_leverage: Some(1),
                ..Default::default()
            }],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta, 0).unwrap();
        assert_eq!(defs[0].price_decimals, 0);
        assert_eq!(defs[0].tick_size, dec!(1));
    }

    #[rstest]
    fn test_parse_perp_instruments_hip3_dex() {
        // HIP-3 dex at index 1: asset_index_base = 100_000 + 1 * 10_000 = 110_000
        let meta = PerpMeta {
            universe: vec![
                PerpAsset {
                    name: "xyz:TSLA".to_string(),
                    sz_decimals: 3,
                    max_leverage: Some(10),
                    only_isolated: None,
                    is_delisted: None,
                    growth_mode: Some("enabled".to_string()),
                    margin_mode: Some("strictIsolated".to_string()),
                },
                PerpAsset {
                    name: "xyz:NVDA".to_string(),
                    sz_decimals: 3,
                    max_leverage: Some(20),
                    only_isolated: None,
                    is_delisted: None,
                    growth_mode: None,
                    margin_mode: None,
                },
            ],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta, 110_000).unwrap();
        assert_eq!(defs.len(), 2);

        // HIP-3 asset: colon in symbol, offset asset index
        assert_eq!(defs[0].symbol, "xyz:TSLA-USD-PERP");
        assert!(defs[0].symbol.contains(':'));
        assert_eq!(defs[0].base, "xyz:TSLA");
        assert_eq!(defs[0].asset_index, 110_000);
        assert!(defs[0].active);

        assert_eq!(defs[1].symbol, "xyz:NVDA-USD-PERP");
        assert_eq!(defs[1].asset_index, 110_001);
    }

    #[rstest]
    #[case("BTC", "BTC")]
    #[case("kPEPE", "kPEPE")]
    #[case("xyz:TSLA", "xyz:TSLA")]
    #[case("dex:STREAMABCD****", "dex:STREAMABCDxxxx")]
    #[case("ABC?", "ABCx")]
    #[case("a*b?c", "axbxc")]
    fn test_sanitize_symbol(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(sanitize_symbol(input), expected);
    }

    #[rstest]
    fn test_parse_spot_instruments_sanitizes_wildcard_token_names() {
        // Hypothetical spot token whose venue name contains `?`. Sanitization
        // must apply to the constructed `symbol` while leaving `raw_symbol`
        // and `base` carrying the venue-official name for wire I/O.
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
                name: "ABC?".to_string(),
                sz_decimals: 4,
                wei_decimals: 4,
                index: 1,
                token_id: "0x2".to_string(),
                is_canonical: true,
                evm_contract: None,
                full_name: None,
                deployer_trading_fee_share: None,
            },
        ];

        let pairs = vec![SpotPair {
            name: "ABC?/USDC".to_string(),
            tokens: [1, 0],
            index: 50,
            is_canonical: true,
        }];

        let meta = SpotMeta {
            tokens,
            universe: pairs,
        };

        let defs = parse_spot_instruments(&meta).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol, "ABCx-USDC-SPOT");
        assert_eq!(defs[0].base, "ABC?");
        assert_eq!(defs[0].quote, "USDC");
    }

    #[rstest]
    fn test_parse_perp_instruments_sanitizes_hip3_wildcards() {
        let meta = PerpMeta {
            universe: vec![PerpAsset {
                name: "dex:STREAMABCD****".to_string(),
                sz_decimals: 3,
                max_leverage: Some(10),
                only_isolated: None,
                is_delisted: None,
                growth_mode: None,
                margin_mode: None,
            }],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta, 110_000).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol, "dex:STREAMABCDxxxx-USD-PERP");
        assert_eq!(defs[0].raw_symbol.as_str(), "dex:STREAMABCD****");
        assert_eq!(defs[0].base.as_str(), "dex:STREAMABCD****");
    }
}

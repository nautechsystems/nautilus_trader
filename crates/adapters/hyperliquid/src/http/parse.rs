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
use nautilus_core::{UUID4, UnixNanos};
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
use ustr::Ustr;

use super::models::{
    AssetPosition, HyperliquidFill, OutcomeMarket, OutcomeMeta, PerpMeta, SpotBalance, SpotMeta,
};
use crate::{
    common::{
        consts::HYPERLIQUID_VENUE,
        enums::{
            HyperliquidFillDirection, HyperliquidOrderStatus as HyperliquidOrderStatusEnum,
            HyperliquidSide, HyperliquidTpSl,
        },
        parse::make_fill_trade_id,
        types::HyperliquidAssetId,
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
    /// HIP-4 binary outcome side token.
    Outcome,
}

/// Outcome-specific metadata carried on [`HyperliquidInstrumentDef`] for HIP-4
/// binary outcome side tokens.
///
/// The venue's `outcomeMeta` payload is partial today (no precision or
/// expiry fields), so unknown values are left as defaults until real venue
/// payloads are available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidOutcomeMetadata {
    /// HIP-4 outcome index (`outcome` field from `outcomeMeta`).
    pub outcome_index: u32,
    /// Side digit (`0` or `1`).
    pub outcome_side: u8,
    /// Outcome market name (for example, "BTC daily").
    pub market_name: Ustr,
    /// Side specification name (for example, "Yes" or "No"); `None` when the
    /// venue payload omits side specs.
    pub side_name: Option<Ustr>,
    /// Venue-supplied description.
    pub description: Option<Ustr>,
    /// Activation timestamp; `0` when the venue payload does not expose it.
    pub activation_ns: UnixNanos,
    /// Expiration timestamp; `0` when the venue payload does not expose it.
    pub expiration_ns: UnixNanos,
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
    /// For outcomes: `#<encoding>` spot-coin form (e.g., "#10").
    pub raw_symbol: Ustr,
    /// Base currency/asset (e.g., "BTC", "PURR").
    pub base: Ustr,
    /// Quote currency (e.g., "USD" for perps, "USDC" for spot).
    pub quote: Ustr,
    /// Market type (perpetual, spot, or outcome).
    pub market_type: HyperliquidMarketType,
    /// Asset index used for order submission.
    /// For perps: index in meta.universe (0, 1, 2, ...).
    /// For spot: 10000 + index in spotMeta.universe.
    /// For outcomes: `100_000_000 + 10 * outcome + side`.
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
    /// Outcome-specific metadata when [`market_type`](Self::market_type) is
    /// [`HyperliquidMarketType::Outcome`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<HyperliquidOutcomeMetadata>,
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
            outcome: None,
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
            outcome: None,
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

// Default precision for HIP-4 outcome side tokens until the venue exposes
// per-market values via `outcomeMeta`. Outcomes settle in `[0, 1]` so 4
// decimals of price granularity (tick `0.0001`) and 2 decimals of size
// granularity (lot `0.01`) are conservative starting values; refine when
// real venue payloads land.
pub const OUTCOME_PRICE_DECIMALS: u32 = 4;
pub const OUTCOME_SIZE_DECIMALS: u32 = 2;

/// Parse outcome instrument definitions from Hyperliquid `outcomeMeta` response.
///
/// Each [`OutcomeMarket`] yields two definitions, one per side (`0` and `1`),
/// modeled as binary outcome side tokens. The Nautilus internal symbol uses
/// the venue's token form (`+<encoding>`), and the wire `raw_symbol` uses the
/// spot-coin form (`#<encoding>`) which is what `l2Book`, `trades`, and `bbo`
/// subscriptions accept.
///
/// Expiry is read from the market's own description when it carries
/// `class:priceBinary`; for outcomes that point at a parent question (`other`
/// or `index:N`), the expiry is inherited from that question's description.
///
/// `side_name` is left unset on the resulting metadata when the venue payload
/// does not supply `sideSpecs`.
pub fn parse_outcome_instruments(
    meta: &OutcomeMeta,
) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    let mut defs = Vec::with_capacity(meta.outcomes.len() * 2);

    for market in &meta.outcomes {
        for side in 0u8..=1u8 {
            defs.push(build_outcome_def(market, side, meta)?);
        }
    }

    Ok(defs)
}

fn build_outcome_def(
    market: &OutcomeMarket,
    side: u8,
    meta: &OutcomeMeta,
) -> Result<HyperliquidInstrumentDef, String> {
    let outcome = market.outcome;
    let asset_id = HyperliquidAssetId::outcome(outcome, side);
    let encoding = asset_id
        .outcome_encoding()
        .ok_or_else(|| format!("Invalid outcome encoding for outcome={outcome} side={side}"))?;

    let token = format!("+{encoding}");
    let coin = format!("#{encoding}");

    let side_name = market
        .side_specs
        .get(usize::from(side))
        .map(|spec| Ustr::from(spec.name.as_str()));

    let description = if market.description.is_empty() {
        None
    } else {
        Some(Ustr::from(market.description.as_str()))
    };

    let expiration_ns = resolve_outcome_expiration_ns(market, meta);

    let outcome = HyperliquidOutcomeMetadata {
        outcome_index: market.outcome,
        outcome_side: side,
        market_name: Ustr::from(market.name.as_str()),
        side_name,
        description,
        activation_ns: UnixNanos::default(),
        expiration_ns,
    };

    Ok(HyperliquidInstrumentDef {
        symbol: Ustr::from(token.as_str()),
        raw_symbol: Ustr::from(coin.as_str()),
        base: Ustr::from(token.as_str()),
        quote: "USDH".into(),
        market_type: HyperliquidMarketType::Outcome,
        asset_index: asset_id.to_raw(),
        price_decimals: OUTCOME_PRICE_DECIMALS,
        size_decimals: OUTCOME_SIZE_DECIMALS,
        tick_size: pow10_neg(OUTCOME_PRICE_DECIMALS),
        lot_size: pow10_neg(OUTCOME_SIZE_DECIMALS),
        max_leverage: None,
        only_isolated: false,
        is_hip3: false,
        active: true,
        outcome: Some(outcome),
        raw_data: serde_json::to_string(market).unwrap_or_default(),
    })
}

fn pow10_neg(decimals: u32) -> Decimal {
    if decimals == 0 {
        return Decimal::ONE;
    }

    // Build 1 / 10^decimals using integer arithmetic
    Decimal::from_i128_with_scale(1, decimals)
}

// Direct binary outcomes carry `expiry:` in their own description. Named
// outcomes (`index:N`) and the `other` fallback inherit expiry from the
// parent question. Returns zero when no expiry can be located.
fn resolve_outcome_expiration_ns(market: &OutcomeMarket, meta: &OutcomeMeta) -> UnixNanos {
    if let Some(ns) = parse_expiry_from_description(&market.description) {
        return ns;
    }

    meta.parent_question(market.outcome)
        .and_then(|q| parse_expiry_from_description(&q.description))
        .unwrap_or_default()
}

fn parse_expiry_from_description(description: &str) -> Option<UnixNanos> {
    description
        .split('|')
        .filter_map(|piece| piece.split_once(':'))
        .find_map(|(key, value)| (key == "expiry").then_some(value))
        .and_then(parse_outcome_expiry_ns)
}

// Parses a Hyperliquid outcome expiry stamp `YYYYMMDD-HHMM` (UTC) to UnixNanos.
fn parse_outcome_expiry_ns(s: &str) -> Option<UnixNanos> {
    let (date_part, time_part) = s.split_once('-')?;
    if date_part.len() != 8 || time_part.len() != 4 {
        return None;
    }

    let year: i32 = date_part[0..4].parse().ok()?;
    let month: u32 = date_part[4..6].parse().ok()?;
    let day: u32 = date_part[6..8].parse().ok()?;
    let hour: u32 = time_part[0..2].parse().ok()?;
    let minute: u32 = time_part[2..4].parse().ok()?;

    let datetime = chrono::NaiveDate::from_ymd_opt(year, month, day)?
        .and_hms_opt(hour, minute, 0)?
        .and_utc();
    let nanos = datetime.timestamp_nanos_opt()?;
    u64::try_from(nanos).ok().map(UnixNanos::from)
}

/// Settlement state for a single HIP-4 outcome side token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutcomeSettlement {
    /// Outcome index from `outcomeMeta`.
    pub outcome_index: u32,
    /// Side token (`0` or `1`).
    pub outcome_side: u8,
    /// Final settlement value: `1` for the winning side, `0` for losing sides.
    pub final_value: u8,
}

/// Derives per-side settlement values from an `outcomeMeta` snapshot.
///
/// Returns one [`OutcomeSettlement`] for every side of every outcome whose
/// resolution can be inferred from the snapshot:
///
/// - For each question with non-empty `settled_named_outcomes`, every named
///   outcome and the fallback are emitted: the winning named outcomes get
///   `Yes -> 1, No -> 0`, every other named outcome and the fallback get
///   `Yes -> 0, No -> 1`.
/// - Standalone outcomes (not referenced by any question) are skipped because
///   the venue does not expose their resolution in `outcomeMeta`. They will
///   need a separate signal (status flag, fill, or position-state event).
///
/// Outcomes referenced by a question that has not yet settled are also
/// skipped. This lets a caller poll `outcomeMeta` and emit settlement events
/// when entries first appear in the result.
#[must_use]
pub fn derive_outcome_settlements(meta: &OutcomeMeta) -> Vec<OutcomeSettlement> {
    let mut settlements = Vec::new();

    for question in &meta.questions {
        if question.settled_named_outcomes.is_empty() {
            continue;
        }

        let losing_sides_won = |outcome_index: u32| -> [OutcomeSettlement; 2] {
            // Named outcome did not win; Yes side -> 0, No side -> 1.
            [
                OutcomeSettlement {
                    outcome_index,
                    outcome_side: 0,
                    final_value: 0,
                },
                OutcomeSettlement {
                    outcome_index,
                    outcome_side: 1,
                    final_value: 1,
                },
            ]
        };

        let winning_sides = |outcome_index: u32| -> [OutcomeSettlement; 2] {
            // Named outcome won; Yes side -> 1, No side -> 0.
            [
                OutcomeSettlement {
                    outcome_index,
                    outcome_side: 0,
                    final_value: 1,
                },
                OutcomeSettlement {
                    outcome_index,
                    outcome_side: 1,
                    final_value: 0,
                },
            ]
        };

        for outcome_index in &question.named_outcomes {
            if question.settled_named_outcomes.contains(outcome_index) {
                settlements.extend(winning_sides(*outcome_index));
            } else {
                settlements.extend(losing_sides_won(*outcome_index));
            }
        }

        // The fallback is the "no named outcome resolved" branch; it loses
        // whenever any named outcome won.
        if let Some(fallback) = question.fallback_outcome {
            settlements.extend(losing_sides_won(fallback));
        }
    }

    settlements
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

/// Returns the HIP-4 outcome settlement currency, registering it on first call.
///
/// Outcome markets settle in USDH (token index 360 on the `USDH/USDC` spot pair
/// `@230`), not USDC. The registration is explicit so the precision is
/// deterministic rather than dependent on whichever caller first triggers
/// `get_currency`'s auto-register path.
pub fn get_usdh_currency() -> Currency {
    Currency::try_from_str("USDH").unwrap_or_else(|| {
        let currency = Currency::new("USDH", 8, 0, "Hyperliquid USD", CurrencyType::Crypto);
        if let Err(e) = Currency::register(currency, false) {
            log::error!("Failed to register USDH currency: {e}");
        }
        currency
    })
}

/// Resolves the commission currency for a fill given the venue's `feeToken` field.
///
/// HIP-4 outcome fills echo the side token (e.g. `+50`) as `feeToken` even when
/// the fee is zero. The side token is not a Nautilus currency and emitting it as
/// the commission currency would leak into `OrderFilled` events and persistence;
/// for outcome side tokens the instrument's quote currency is always used, even
/// when another adapter path (such as spot-balance parsing) has registered the
/// side token in the global registry. Non-zero side-token fees error: the venue
/// does not denominate fees in side tokens. Other unknown tokens fall back to
/// the instrument's quote currency only when the fee is zero.
///
/// # Errors
///
/// Returns an error when an outcome side token carries a non-zero fee, or when
/// `fee_token` cannot be resolved and `fee_amount` is non-zero.
pub fn resolve_fee_currency(
    fee_token: &str,
    fee_amount: Decimal,
    instrument: &dyn Instrument,
) -> anyhow::Result<Currency> {
    if is_outcome_side_token(fee_token) {
        if !fee_amount.is_zero() {
            anyhow::bail!(
                "Outcome side token '{fee_token}' carried a non-zero fee {fee_amount}; \
                 venue does not denominate fees in side tokens",
            );
        }
        return Ok(instrument.quote_currency());
    }

    if let Some(currency) = Currency::try_from_str(fee_token) {
        return Ok(currency);
    }

    if fee_amount.is_zero() {
        let fallback = instrument.quote_currency();
        log::debug!(
            "Unregistered fee token '{fee_token}' on zero-fee fill for {}; using {fallback} as fallback",
            instrument.id(),
        );
        return Ok(fallback);
    }

    anyhow::bail!("Unknown fee token '{fee_token}' with non-zero fee {fee_amount}")
}

fn is_outcome_side_token(symbol: &str) -> bool {
    let Some(rest) = symbol.strip_prefix('+') else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
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
            let outcome = def.outcome.as_ref()?;
            let currency = get_usdh_currency();

            Some(InstrumentAny::BinaryOption(BinaryOption::new(
                instrument_id,
                raw_symbol,
                AssetClass::Alternative,
                currency,
                outcome.activation_ns,
                outcome.expiration_ns,
                def.price_decimals as u8,
                def.size_decimals as u8,
                price_increment,
                size_increment,
                outcome.side_name,
                outcome.description,
                None, // max_quantity
                None, // min_quantity
                None, // max_notional
                None, // min_notional
                None, // max_price
                None, // min_price
                None, // margin_init
                None, // margin_maint
                None, // maker_fee
                None, // taker_fee
                None, // info
                ts_init,
                ts_init,
            )))
        }
    }
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

    let fee_currency = resolve_fee_currency(fill.fee_token.as_str(), fee_amount, instrument)?;
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
        super::models::{
            HyperliquidL2Book, OutcomeMarket, OutcomeMeta, OutcomeQuestion, OutcomeSideSpec,
            PerpAsset, SpotPair, SpotToken,
        },
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

    #[rstest]
    fn test_parse_outcome_instruments_emits_both_sides() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 1,
                name: "BTC daily".to_string(),
                description: "BTC settles above strike at 06:00 UTC".to_string(),
                side_specs: vec![
                    OutcomeSideSpec {
                        name: "Yes".to_string(),
                    },
                    OutcomeSideSpec {
                        name: "No".to_string(),
                    },
                ],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        assert_eq!(defs.len(), 2);

        let yes = &defs[0];
        assert_eq!(yes.symbol.as_str(), "+10");
        assert_eq!(yes.raw_symbol.as_str(), "#10");
        assert_eq!(yes.market_type, HyperliquidMarketType::Outcome);
        assert_eq!(yes.asset_index, 100_000_010);
        assert_eq!(yes.price_decimals, OUTCOME_PRICE_DECIMALS);
        assert_eq!(yes.size_decimals, OUTCOME_SIZE_DECIMALS);
        assert_eq!(yes.tick_size, dec!(0.0001));
        assert_eq!(yes.lot_size, dec!(0.01));
        assert_eq!(yes.quote.as_str(), "USDH");
        assert!(yes.active);

        let yes_meta = yes.outcome.as_ref().unwrap();
        assert_eq!(yes_meta.outcome_index, 1);
        assert_eq!(yes_meta.outcome_side, 0);
        assert_eq!(yes_meta.market_name.as_str(), "BTC daily");
        assert_eq!(yes_meta.side_name.unwrap().as_str(), "Yes");
        assert_eq!(
            yes_meta.description.unwrap().as_str(),
            "BTC settles above strike at 06:00 UTC"
        );

        let no = &defs[1];
        assert_eq!(no.symbol.as_str(), "+11");
        assert_eq!(no.raw_symbol.as_str(), "#11");
        assert_eq!(no.asset_index, 100_000_011);
        let no_meta = no.outcome.as_ref().unwrap();
        assert_eq!(no_meta.outcome_side, 1);
        assert_eq!(no_meta.side_name.unwrap().as_str(), "No");
    }

    #[rstest]
    fn test_parse_outcome_instruments_handles_missing_side_specs() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 5,
                name: "Recurring".to_string(),
                description: String::new(),
                side_specs: vec![],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        assert_eq!(defs.len(), 2);

        for def in &defs {
            let outcome = def.outcome.as_ref().unwrap();
            assert!(outcome.side_name.is_none());
            assert!(outcome.description.is_none());
        }

        assert_eq!(defs[0].asset_index, 100_000_050);
        assert_eq!(defs[1].asset_index, 100_000_051);
    }

    #[rstest]
    fn test_get_usdh_currency_registers_with_explicit_precision() {
        let currency = get_usdh_currency();
        assert_eq!(currency.code.as_str(), "USDH");
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.currency_type, CurrencyType::Crypto);

        // Repeated calls return the same registered currency
        let again = get_usdh_currency();
        assert_eq!(again, currency);
        assert!(Currency::try_from_str("USDH").is_some());
    }

    #[rstest]
    fn test_create_instrument_from_def_outcome_emits_binary_option() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 2,
                name: "Recurring BTC".to_string(),
                description: "Daily settlement".to_string(),
                side_specs: vec![
                    OutcomeSideSpec {
                        name: "Yes".to_string(),
                    },
                    OutcomeSideSpec {
                        name: "No".to_string(),
                    },
                ],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        let instrument = create_instrument_from_def(&defs[0], UnixNanos::default()).unwrap();

        match instrument {
            InstrumentAny::BinaryOption(bo) => {
                assert_eq!(bo.id.symbol.as_str(), "+20");
                assert_eq!(bo.raw_symbol.as_str(), "#20");
                assert_eq!(bo.asset_class, AssetClass::Alternative);
                assert_eq!(bo.currency.code.as_str(), "USDH");
                assert_eq!(bo.price_precision, OUTCOME_PRICE_DECIMALS as u8);
                assert_eq!(bo.size_precision, OUTCOME_SIZE_DECIMALS as u8);
                assert_eq!(bo.outcome.unwrap().as_str(), "Yes");
                assert_eq!(bo.description.unwrap().as_str(), "Daily settlement");
            }
            other => panic!("Expected BinaryOption, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_fill_report_outcome_round_trip() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 42,
                name: "BTC daily".to_string(),
                description: "BTC settles above strike at 06:00 UTC".to_string(),
                side_specs: vec![
                    OutcomeSideSpec {
                        name: "Yes".to_string(),
                    },
                    OutcomeSideSpec {
                        name: "No".to_string(),
                    },
                ],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        let yes = create_instrument_from_def(&defs[0], UnixNanos::default()).unwrap();
        assert_eq!(yes.id().symbol.as_str(), "+420");

        let fill = HyperliquidFill {
            coin: Ustr::from("#420"),
            px: "0.5500".to_string(),
            sz: "1000.00".to_string(),
            side: HyperliquidSide::Buy,
            time: 1_704_470_400_000,
            start_position: "0.00".to_string(),
            dir: HyperliquidFillDirection::OpenLong,
            closed_pnl: "0.0".to_string(),
            hash: "0xfeed".to_string(),
            oid: 99_001,
            crossed: true,
            fee: "0.0".to_string(),
            fee_token: Ustr::from("+420"),
        };

        let account_id = AccountId::from("HYPERLIQUID-001");
        let report = parse_fill_report(&fill, &yes, account_id, UnixNanos::default()).unwrap();

        // Zero-fee outcome fills resolve commission to the instrument's quote
        // currency (USDH) rather than the side token, so downstream OrderFilled
        // events and persistence carry a registered currency.
        assert_eq!(report.commission.currency.code.as_str(), "USDH");
        assert!(report.commission.as_decimal().is_zero());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.last_qty.as_decimal(), dec!(1000));
        assert_eq!(report.last_px.as_decimal(), dec!(0.55));
    }

    #[rstest]
    fn test_resolve_fee_currency_outcome_token_returns_quote_even_when_registered() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 88,
                name: "Edge".to_string(),
                description: String::new(),
                side_specs: vec![],
            }],
            questions: vec![],
        };
        let defs = parse_outcome_instruments(&meta).unwrap();
        let yes = create_instrument_from_def(&defs[0], UnixNanos::default()).unwrap();

        // Simulate another adapter path (e.g. spot balance parsing) having already
        // registered the side token in the global currency registry.
        let _ = get_currency("+880");
        assert!(Currency::try_from_str("+880").is_some());

        let currency = resolve_fee_currency("+880", Decimal::ZERO, &yes)
            .expect("zero-fee outcome side token must resolve to quote currency");
        assert_eq!(currency.code.as_str(), "USDH");

        let err = resolve_fee_currency("+880", dec!(0.01), &yes).unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("Outcome side token '+880'"));
        assert!(err_msg.contains("non-zero fee"));
    }

    #[rstest]
    #[case("+50", true)]
    #[case("+0", true)]
    #[case("+880", true)]
    #[case("", false)]
    #[case("+", false)]
    #[case("+abc", false)]
    #[case("+50a", false)]
    #[case("#50", false)]
    #[case("USDC", false)]
    #[case("-50", false)]
    fn test_is_outcome_side_token(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_outcome_side_token(input), expected);
    }

    #[rstest]
    fn test_resolve_fee_currency_falls_back_to_quote_when_unregistered_and_zero_fee() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 77,
                name: "Edge".to_string(),
                description: String::new(),
                side_specs: vec![],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        let no = create_instrument_from_def(&defs[1], UnixNanos::default()).unwrap();

        // Use a token that the venue would not normally emit; the helper must still
        // return the instrument's quote currency on a zero-fee fill.
        let currency = resolve_fee_currency("+UNREGISTERED-TOKEN", Decimal::ZERO, &no)
            .expect("zero-fee fallback should succeed");
        assert_eq!(currency.code.as_str(), "USDH");

        let err = resolve_fee_currency("+UNREGISTERED-TOKEN", dec!(0.01), &no).unwrap_err();
        assert!(err.to_string().contains("non-zero fee"));
    }

    #[rstest]
    fn test_parse_outcome_expiry_ns_round_trip() {
        // 2026-05-08 06:00:00 UTC == 1778652000 seconds since epoch
        let ns = parse_outcome_expiry_ns("20260508-0600").unwrap();
        assert_eq!(ns.as_u64(), 1_778_220_000_000_000_000);
    }

    #[rstest]
    #[case("")]
    #[case("20260508")]
    #[case("20260508-")]
    #[case("20260508-0600 ")]
    #[case("2026-05-08-06-00")]
    #[case("20261308-0600")]
    fn test_parse_outcome_expiry_ns_rejects_bad_input(#[case] input: &str) {
        assert!(parse_outcome_expiry_ns(input).is_none());
    }

    #[rstest]
    fn test_parse_outcome_instruments_pulls_expiry_from_price_binary() {
        let meta = OutcomeMeta {
            outcomes: vec![OutcomeMarket {
                outcome: 5,
                name: "Recurring".to_string(),
                description:
                    "class:priceBinary|underlying:BTC|expiry:20260508-0600|targetPrice:81041|period:1d"
                        .to_string(),
                side_specs: vec![
                    OutcomeSideSpec {
                        name: "Yes".to_string(),
                    },
                    OutcomeSideSpec {
                        name: "No".to_string(),
                    },
                ],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        let yes_meta = defs[0].outcome.as_ref().unwrap();
        assert_eq!(yes_meta.expiration_ns.as_u64(), 1_778_220_000_000_000_000);
    }

    #[rstest]
    fn test_parse_outcome_instruments_inherits_expiry_from_parent_question() {
        // outcome=7 has `index:0` description and is referenced by question 0's
        // `named_outcomes`. outcome=6 has `other` description and is the
        // `fallback_outcome`. Both should pick up the question's expiry.
        let meta = OutcomeMeta {
            outcomes: vec![
                OutcomeMarket {
                    outcome: 6,
                    name: "Recurring Fallback".to_string(),
                    description: "other".to_string(),
                    side_specs: vec![],
                },
                OutcomeMarket {
                    outcome: 7,
                    name: "Recurring Named Outcome".to_string(),
                    description: "index:0".to_string(),
                    side_specs: vec![],
                },
            ],
            questions: vec![OutcomeQuestion {
                question: 0,
                name: "Recurring".to_string(),
                description:
                    "class:priceBucket|underlying:BTC|expiry:20260508-0600|priceThresholds:79303,82540|period:1d"
                        .to_string(),
                fallback_outcome: Some(6),
                named_outcomes: vec![7, 8, 9],
                settled_named_outcomes: vec![],
            }],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        let expected_ns: u64 = 1_778_220_000_000_000_000;

        for def in &defs {
            let outcome = def.outcome.as_ref().unwrap();
            assert_eq!(
                outcome.expiration_ns.as_u64(),
                expected_ns,
                "outcome {} side {} should inherit expiry",
                outcome.outcome_index,
                outcome.outcome_side,
            );
        }
    }

    #[rstest]
    fn test_derive_outcome_settlements_returns_empty_when_no_questions() {
        let meta = OutcomeMeta {
            outcomes: vec![],
            questions: vec![],
        };
        assert!(derive_outcome_settlements(&meta).is_empty());
    }

    #[rstest]
    fn test_derive_outcome_settlements_returns_empty_when_no_questions_settled() {
        let meta = OutcomeMeta {
            outcomes: vec![],
            questions: vec![OutcomeQuestion {
                question: 0,
                name: "Recurring".to_string(),
                description: "class:priceBucket|expiry:20260508-0600".to_string(),
                fallback_outcome: Some(6),
                named_outcomes: vec![7, 8, 9],
                settled_named_outcomes: vec![],
            }],
        };

        assert!(derive_outcome_settlements(&meta).is_empty());
    }

    #[rstest]
    fn test_derive_outcome_settlements_marks_winners_losers_and_fallback() {
        let meta = OutcomeMeta {
            outcomes: vec![],
            questions: vec![OutcomeQuestion {
                question: 0,
                name: "Recurring".to_string(),
                description: "class:priceBucket|expiry:20260508-0600".to_string(),
                fallback_outcome: Some(6),
                named_outcomes: vec![7, 8, 9],
                settled_named_outcomes: vec![8],
            }],
        };

        let settlements = derive_outcome_settlements(&meta);
        let lookup: ahash::AHashMap<(u32, u8), u8> = settlements
            .into_iter()
            .map(|s| ((s.outcome_index, s.outcome_side), s.final_value))
            .collect();

        // Winning named outcome 8: Yes -> 1, No -> 0
        assert_eq!(lookup[&(8, 0)], 1);
        assert_eq!(lookup[&(8, 1)], 0);

        // Losing named outcomes 7, 9 and fallback 6: Yes -> 0, No -> 1
        for losing in [7, 9, 6] {
            assert_eq!(lookup[&(losing, 0)], 0, "outcome {losing} Yes side");
            assert_eq!(lookup[&(losing, 1)], 1, "outcome {losing} No side");
        }

        assert_eq!(lookup.len(), 8);
    }

    #[rstest]
    fn test_parse_outcome_meta_question_settlement_round_trip() {
        let json = r#"{
            "outcomes": [{"outcome": 5, "name": "Recurring", "description": "class:priceBinary|expiry:20260508-0600", "sideSpecs": []}],
            "questions": [{
                "question": 0,
                "name": "Recurring",
                "description": "class:priceBucket|expiry:20260508-0600",
                "fallbackOutcome": 6,
                "namedOutcomes": [7, 8, 9],
                "settledNamedOutcomes": [8]
            }]
        }"#;

        let meta: OutcomeMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.questions.len(), 1);
        let q = &meta.questions[0];
        assert_eq!(q.fallback_outcome, Some(6));
        assert_eq!(q.named_outcomes, vec![7, 8, 9]);
        assert_eq!(q.settled_named_outcomes, vec![8]);

        assert!(meta.parent_question(7).is_some());
        assert!(meta.parent_question(6).is_some());
        assert!(meta.parent_question(99).is_none());
    }
}

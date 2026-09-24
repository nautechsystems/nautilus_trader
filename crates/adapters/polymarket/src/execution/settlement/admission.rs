// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the
//  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
//  either express or implied. See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! The status-aware admission boundary shared by WebSocket and REST trade evidence.
//!
//! Every payload, from either transport, is normalized and validated here before the settlement
//! registry acts on it: exact account, trade, order, instrument, ownership, side, and liquidity
//! identity; fill economics representable as domain values; and a venue match timestamp in place
//! of the local clock. Failed evidence admits complete owned-leg identity without fill economics.
//! Status-only order or position reports cannot construct fill economics through this boundary.

use std::fmt::Display;

use anyhow::Context;
use nautilus_core::{
    UUID4, UnixNanos, collections::AtomicMap, correctness::check_valid_string_ascii,
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::FillReport,
    types::{Currency, Money, Price, Quantity, fixed::FIXED_PRECISION},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        enums::{
            PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOutcome, PolymarketSignerType,
            PolymarketTradeStatus,
        },
        models::{PolymarketMakerOrder, is_owned_by_account},
        parse::parse_decimal_exact,
    },
    execution::{
        parse::{
            composite_trade_id_value, compute_commission, determine_order_side,
            instrument_fee_exponent, instrument_taker_fee, parse_timestamp,
        },
        reconciliation::{
            checked_venue_order_id, validate_instrument_binding, validate_price_evidence,
            validate_quantity_evidence,
        },
    },
    http::models::PolymarketTradeReport,
    websocket::messages::PolymarketUserTrade,
};

/// Account and instrument context required to admit trade evidence for the configured account.
pub(crate) struct AdmissionContext<'a> {
    pub signer_type: PolymarketSignerType,
    pub user_address: &'a str,
    pub api_key: &'a str,
    pub pusd: Currency,
    pub instruments: &'a AtomicMap<Ustr, InstrumentAny>,
}

/// Trade evidence from either transport, before shared validation.
#[derive(Clone, Copy)]
pub(crate) enum TradeEvidence<'a> {
    Stream(&'a PolymarketUserTrade),
    Rest(&'a PolymarketTradeReport),
}

/// Why trade evidence did not pass the shared admission boundary.
#[derive(Debug)]
pub(crate) enum AdmissionError {
    /// The trade references an instrument that is not loaded yet.
    UnknownInstrument(Ustr),
    /// The payload holds no legs owned by the configured account.
    Unowned,
    /// Fill evidence lacks a valid venue match timestamp; every other check passed.
    Untimestamped(anyhow::Error),
    /// Evidence failed shared validation.
    Invalid(anyhow::Error),
}

impl Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownInstrument(token) => write!(f, "instrument {token} is not loaded"),
            Self::Unowned => write!(f, "no legs owned by the account"),
            Self::Untimestamped(e) | Self::Invalid(e) => write!(f, "{e}"),
        }
    }
}

/// One admitted owned leg of a trade, validated by the shared boundary.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdmittedLeg {
    pub venue_order_id: VenueOrderId,
    pub trade_id: TradeId,
    pub instrument_id: InstrumentId,
    pub order_side: OrderSide,
    pub liquidity_side: LiquiditySide,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub commission: Money,
    pub ts_event: UnixNanos,
}

impl AdmittedLeg {
    /// Returns the fill report for this leg, without a client order ID.
    pub(crate) fn fill_report(&self, account_id: AccountId, ts_init: UnixNanos) -> FillReport {
        FillReport {
            account_id,
            instrument_id: self.instrument_id,
            venue_order_id: self.venue_order_id,
            trade_id: self.trade_id,
            order_side: self.order_side,
            last_qty: self.last_qty,
            last_px: self.last_px,
            commission: self.commission.clone(),
            liquidity_side: self.liquidity_side,
            avg_px: None,
            report_id: UUID4::new(),
            ts_event: self.ts_event,
            ts_init,
            client_order_id: None,
            venue_position_id: None,
        }
    }
}

/// Admitted trade evidence from either transport.
#[derive(Debug)]
pub(crate) struct AdmittedTrade {
    pub venue_trade_id: String,
    pub status: PolymarketTradeStatus,
    pub legs: Vec<AdmittedLeg>,
}

impl AdmittedTrade {
    /// Returns `true` when the evidence carries fill economics; failed evidence does not.
    pub(crate) fn has_economics(&self) -> bool {
        self.status != PolymarketTradeStatus::Failed
    }
}

/// Admits trade evidence through the one shared validation boundary.
///
/// Every owned leg must validate before any effect is attempted; a payload whose owned legs do
/// not all validate is rejected whole. The adapter treats each payload as the complete trade
/// entity (taker plus full maker set), the assumption RFC #4876 requires venue evidence to
/// confirm before merge.
pub(crate) fn admit_trade_evidence(
    evidence: TradeEvidence<'_>,
    ctx: &AdmissionContext<'_>,
) -> Result<AdmittedTrade, AdmissionError> {
    admit_owned_legs(evidence, ctx, None)
}

/// Admits only the owned legs of the given venue orders, for reports scoped to part of a trade.
///
/// Legs of other orders are neither validated nor admitted, so malformed evidence outside the
/// scope does not block the report.
pub(crate) fn admit_trade_legs(
    evidence: TradeEvidence<'_>,
    ctx: &AdmissionContext<'_>,
    venue_order_ids: &[&str],
) -> Result<AdmittedTrade, AdmissionError> {
    admit_owned_legs(evidence, ctx, Some(venue_order_ids))
}

fn admit_owned_legs(
    evidence: TradeEvidence<'_>,
    ctx: &AdmissionContext<'_>,
    venue_order_ids: Option<&[&str]>,
) -> Result<AdmittedTrade, AdmissionError> {
    let is_selected = |order_id: &str| venue_order_ids.is_none_or(|ids| ids.contains(&order_id));

    let trade = match evidence {
        TradeEvidence::Stream(stream) => {
            normalize_stream(stream).map_err(AdmissionError::Invalid)?
        }
        TradeEvidence::Rest(rest) => normalize_rest(rest),
    };

    let failed = trade.status == PolymarketTradeStatus::Failed;
    let mut legs = Vec::new();

    if trade.trader_side == PolymarketLiquiditySide::Taker && is_selected(trade.taker_order_id) {
        legs.push(admit_owned_taker_leg(&trade, ctx, failed)?);
    }

    let mut admitted_maker_order_ids: Vec<String> = Vec::new();

    for maker_order in trade.maker_orders {
        if !maker_order.is_owned_by(ctx.user_address, ctx.api_key, ctx.signer_type)
            || !is_selected(&maker_order.order_id)
        {
            continue;
        }

        if admitted_maker_order_ids.contains(&maker_order.order_id) {
            return Err(AdmissionError::Invalid(anyhow::anyhow!(
                "maker order {} appears more than once in trade {}",
                maker_order.order_id,
                trade.id,
            )));
        }

        admitted_maker_order_ids.push(maker_order.order_id.clone());

        let instrument = ctx
            .instruments
            .get_cloned(&maker_order.asset_id)
            .ok_or(AdmissionError::UnknownInstrument(maker_order.asset_id))?;
        legs.push(
            admit_maker_leg(&trade, maker_order, &instrument, ctx, failed)
                .map_err(AdmissionError::Invalid)?,
        );
    }

    if legs.is_empty() {
        return Err(AdmissionError::Unowned);
    }

    if !failed && trade.ts_event.is_none() {
        return Err(AdmissionError::Untimestamped(anyhow::anyhow!(
            "trade {} has no valid venue match timestamp (match_time={})",
            trade.id,
            trade.match_time_source,
        )));
    }

    Ok(AdmittedTrade {
        venue_trade_id: trade.id.to_string(),
        status: trade.status,
        legs,
    })
}

/// Normalized trade fields common to the WebSocket and REST payloads.
///
/// `from_rest` marks REST evidence, whose maker orders always carry a side and whose prices keep
/// their exact wire precision rather than the instrument precision. `match_time_source` keeps the
/// raw `match_time` for validation messages, and `ts_event` is the venue match timestamp parsed
/// from it, with the WebSocket `timestamp` as fallback.
struct NormalizedTrade<'a> {
    id: &'a str,
    status: PolymarketTradeStatus,
    trader_side: PolymarketLiquiditySide,
    side: PolymarketOrderSide,
    market: &'a str,
    asset_id: Ustr,
    outcome: PolymarketOutcome,
    taker_order_id: &'a str,
    taker_size: Decimal,
    taker_price: Decimal,
    taker_maker_address: String,
    taker_owner: String,
    maker_orders: &'a [PolymarketMakerOrder],
    from_rest: bool,
    match_time_source: &'a str,
    ts_event: Option<UnixNanos>,
}

fn normalize_stream(trade: &PolymarketUserTrade) -> anyhow::Result<NormalizedTrade<'_>> {
    let ts_event = parse_timestamp(&trade.match_time).or_else(|| {
        trade
            .timestamp
            .parse::<u64>()
            .ok()
            .and_then(|ms| ms.checked_mul(1_000_000))
            .map(UnixNanos::from)
    });

    Ok(NormalizedTrade {
        id: &trade.id,
        status: trade.status,
        trader_side: trade.trader_side,
        side: trade.side,
        market: trade.market.as_str(),
        asset_id: trade.asset_id,
        outcome: trade.outcome,
        taker_order_id: &trade.taker_order_id,
        taker_size: parse_decimal_exact(&trade.size)
            .with_context(|| format!("trade {} size is not a decimal", trade.id))?,
        taker_price: parse_decimal_exact(&trade.price)
            .with_context(|| format!("trade {} price is not a decimal", trade.id))?,
        taker_maker_address: trade.maker_address.to_string(),
        taker_owner: trade.owner.to_string(),
        maker_orders: &trade.maker_orders,
        from_rest: false,
        match_time_source: &trade.match_time,
        ts_event,
    })
}

fn normalize_rest(trade: &PolymarketTradeReport) -> NormalizedTrade<'_> {
    NormalizedTrade {
        id: &trade.id,
        status: trade.status,
        trader_side: trade.trader_side,
        side: trade.side,
        market: trade.market.as_str(),
        asset_id: trade.asset_id,
        outcome: trade.outcome,
        taker_order_id: &trade.taker_order_id,
        taker_size: trade.size,
        taker_price: trade.price,
        taker_maker_address: trade.maker_address.clone(),
        taker_owner: trade.owner.clone(),
        maker_orders: &trade.maker_orders,
        from_rest: true,
        match_time_source: &trade.match_time,
        ts_event: parse_timestamp(&trade.match_time),
    }
}

fn admit_owned_taker_leg(
    trade: &NormalizedTrade<'_>,
    ctx: &AdmissionContext<'_>,
    failed: bool,
) -> Result<AdmittedLeg, AdmissionError> {
    if !is_owned_by_account(
        &trade.taker_maker_address,
        &trade.taker_owner,
        ctx.user_address,
        ctx.api_key,
        ctx.signer_type,
    ) {
        return Err(AdmissionError::Unowned);
    }

    let instrument = ctx
        .instruments
        .get_cloned(&trade.asset_id)
        .ok_or(AdmissionError::UnknownInstrument(trade.asset_id))?;
    admit_taker_leg(trade, &instrument, ctx, failed).map_err(AdmissionError::Invalid)
}

fn admit_taker_leg(
    trade: &NormalizedTrade<'_>,
    instrument: &InstrumentAny,
    ctx: &AdmissionContext<'_>,
    failed: bool,
) -> anyhow::Result<AdmittedLeg> {
    validate_instrument_binding(instrument, trade.market, trade.outcome)?;

    let venue_order_id = checked_venue_order_id(trade.taker_order_id, "taker trade")?;
    let trade_id = checked_trade_id(trade.id, "taker trade")?;
    let order_side = OrderSide::from(trade.side);

    let (last_qty, last_px, commission) = if failed {
        zero_economics(instrument, ctx)
    } else {
        let size_prec = instrument.size_precision();
        validate_quantity_evidence(
            trade.taker_size,
            size_prec,
            &format!("taker trade {} size", trade.id),
            false,
        )?;
        let last_px = validate_price(
            trade,
            trade.taker_price,
            instrument,
            &format!("taker trade {} price", trade.id),
        )?;
        let commission = compute_commission(
            instrument_taker_fee(instrument)?,
            instrument_fee_exponent(instrument)?,
            trade.taker_size,
            trade.taker_price,
            LiquiditySide::Taker,
        )
        .with_context(|| format!("taker trade {} commission overflow", trade.id))?;
        (
            Quantity::from_decimal_dp(trade.taker_size, size_prec)
                .with_context(|| format!("taker trade {} quantity overflow", trade.id))?,
            last_px,
            Money::from_decimal(commission, ctx.pusd.clone()).with_context(|| {
                format!(
                    "failed to represent commission {commission} for taker trade {} as Money",
                    trade.id
                )
            })?,
        )
    };

    Ok(AdmittedLeg {
        venue_order_id,
        trade_id,
        instrument_id: instrument.id(),
        order_side,
        liquidity_side: LiquiditySide::Taker,
        last_qty,
        last_px,
        commission,
        ts_event: trade.ts_event.unwrap_or_default(),
    })
}

fn admit_maker_leg(
    trade: &NormalizedTrade<'_>,
    maker_order: &PolymarketMakerOrder,
    instrument: &InstrumentAny,
    ctx: &AdmissionContext<'_>,
    failed: bool,
) -> anyhow::Result<AdmittedLeg> {
    validate_instrument_binding(instrument, trade.market, maker_order.outcome)?;

    let derived_side = determine_order_side(
        trade.trader_side,
        trade.side,
        trade.asset_id.as_str(),
        maker_order.asset_id.as_str(),
    );

    match maker_order.side {
        Some(provider_side) => anyhow::ensure!(
            OrderSide::from(provider_side) == derived_side,
            "provider maker order {} side {provider_side} contradicts derived side {derived_side}",
            maker_order.order_id,
        ),
        None => anyhow::ensure!(
            !trade.from_rest,
            "REST maker order {} is missing side",
            maker_order.order_id,
        ),
    }

    let venue_order_id = checked_venue_order_id(
        &maker_order.order_id,
        &format!("maker order in trade {}", trade.id),
    )?;

    // The composite ID slices bytes, so the trade ID source must be ASCII first
    check_valid_string_ascii(trade.id, "trade.id")
        .with_context(|| format!("maker trade {} has invalid trade ID source", trade.id))?;
    let trade_id = checked_trade_id(
        &composite_trade_id_value(trade.id, &maker_order.order_id),
        &format!("maker trade {} composite", trade.id),
    )?;

    let (last_qty, last_px, commission) = if failed {
        zero_economics(instrument, ctx)
    } else {
        let size_prec = instrument.size_precision();
        validate_quantity_evidence(
            maker_order.matched_amount,
            size_prec,
            &format!("maker order {} matched amount", maker_order.order_id),
            false,
        )?;
        let last_px = validate_price(
            trade,
            maker_order.price,
            instrument,
            &format!("maker order {} price", maker_order.order_id),
        )?;
        (
            Quantity::from_decimal_dp(maker_order.matched_amount, size_prec).with_context(
                || format!("maker order {} quantity overflow", maker_order.order_id),
            )?,
            last_px,
            Money::zero(ctx.pusd.clone()),
        )
    };

    Ok(AdmittedLeg {
        venue_order_id,
        trade_id,
        instrument_id: instrument.id(),
        order_side: derived_side,
        liquidity_side: LiquiditySide::Maker,
        last_qty,
        last_px,
        commission,
        ts_event: trade.ts_event.unwrap_or_default(),
    })
}

fn checked_trade_id(value: &str, evidence: &str) -> anyhow::Result<TradeId> {
    TradeId::new_checked(value)
        .with_context(|| format!("{evidence} has invalid trade ID {value:?}"))
}

fn validate_price(
    trade: &NormalizedTrade<'_>,
    value: Decimal,
    instrument: &InstrumentAny,
    field: &str,
) -> anyhow::Result<Price> {
    if trade.from_rest {
        return validate_historical_price_evidence(value, field);
    }

    validate_price_evidence(value, instrument.price_precision(), field)?;
    Price::from_decimal_dp(value, instrument.price_precision())
        .with_context(|| format!("{field} {value} overflow"))
}

fn validate_historical_price_evidence(value: Decimal, field: &str) -> anyhow::Result<Price> {
    anyhow::ensure!(
        value > Decimal::ZERO && value < Decimal::ONE,
        "{field} {value} must be greater than zero and less than one",
    );
    let evidence = value.normalize();
    anyhow::ensure!(
        evidence.scale() <= u32::from(FIXED_PRECISION),
        "historical {field} {value} exceeds the maximum representable price precision of {FIXED_PRECISION} decimals",
    );
    let price = Price::from_decimal(evidence)
        .with_context(|| format!("failed to represent historical {field} {value}"))?;
    anyhow::ensure!(
        price.as_decimal() == value,
        "historical {field} {value} is not exactly representable as a price",
    );
    Ok(price)
}

fn zero_economics(
    instrument: &InstrumentAny,
    ctx: &AdmissionContext<'_>,
) -> (Quantity, Price, Money) {
    (
        Quantity::zero(instrument.size_precision()),
        Price::zero(instrument.price_precision()),
        Money::zero(ctx.pusd.clone()),
    )
}

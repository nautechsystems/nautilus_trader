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
use rust_decimal::{Decimal, RoundingStrategy};
use ustr::Ustr;

use crate::{
    common::{
        consts::USDC_DECIMALS,
        enums::{
            PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOutcome, PolymarketSignerType,
            PolymarketTradeStatus,
        },
        models::{PolymarketMakerOrder, is_owned_by_account},
        parse::parse_decimal_exact,
    },
    execution::{
        parse::{
            composite_trade_id_value, compute_commission, compute_commission_unrounded,
            determine_order_side, instrument_fee_exponent, instrument_taker_fee, parse_timestamp,
        },
        reconciliation::{
            checked_venue_order_id, validate_instrument_binding, validate_price_evidence,
            validate_quantity_evidence,
        },
    },
    http::models::PolymarketTradeReport,
    websocket::messages::PolymarketUserTrade,
};

/// Fee rate and exponent used to compute one taker leg's commission at its first registry admission.
///
/// Later admissions of that leg reuse this basis with the incoming wire size and price. The basis
/// is local instrument metadata, not a venue-reported fee, and it does not prove the venue charge.
/// A schedule that was missing or wrong at first admission stays on that fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TakerFeeBasis {
    pub rate: Decimal,
    pub exponent: Decimal,
}

pub(crate) trait TakerFeeBasisLookup {
    fn taker_fee_basis(
        &self,
        venue_trade_id: &str,
        venue_order_id: &VenueOrderId,
    ) -> Option<TakerFeeBasis>;
}

/// Account and instrument context required to admit trade evidence for the configured account.
pub(crate) struct AdmissionContext<'a> {
    pub signer_type: PolymarketSignerType,
    pub user_address: &'a str,
    pub api_key: &'a str,
    pub pusd: Currency,
    pub instruments: &'a AtomicMap<Ustr, InstrumentAny>,
    pub fee_bases: Option<&'a dyn TakerFeeBasisLookup>,
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
///
/// `taker_fee_basis` is the rate and exponent used to compute a non-failed taker commission.
/// Maker and failed legs leave it unset.
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
    pub taker_fee_basis: Option<TakerFeeBasis>,
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
/// their exact wire precision rather than the instrument precision. A price off the instrument
/// precision only by settlement rounding snaps to it on both transports. `match_time_source`
/// keeps the raw `match_time` for validation messages, and `ts_event` is the venue match
/// timestamp parsed from it, with the WebSocket `timestamp` as fallback.
/// Counterparty executions used to aggregate a taker fill allow historical price precision on
/// both transports; the resulting fill must preserve the settlement notional.
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
    let matches = if failed {
        Vec::new()
    } else {
        validate_taker_matches(trade, &instrument, ctx)?
    };

    admit_taker_leg(trade, &instrument, ctx, failed, &matches).map_err(AdmissionError::Invalid)
}

fn admit_taker_leg(
    trade: &NormalizedTrade<'_>,
    instrument: &InstrumentAny,
    ctx: &AdmissionContext<'_>,
    failed: bool,
    matches: &[(Decimal, Decimal)],
) -> anyhow::Result<AdmittedLeg> {
    validate_instrument_binding(instrument, trade.market, trade.outcome)?;

    let venue_order_id = checked_venue_order_id(trade.taker_order_id, "taker trade")?;
    let trade_id = checked_trade_id(trade.id, "taker trade")?;
    let order_side = OrderSide::from(trade.side);

    let (last_qty, last_px, commission, taker_fee_basis) = if failed {
        let (last_qty, last_px, commission) = zero_economics(instrument, ctx);
        (last_qty, last_px, commission, None)
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
            trade.taker_size,
            instrument,
            &format!("taker trade {} price", trade.id),
        )?;
        // Fee inputs are the wire size and price; snapped last_px can cross the 5-decimal floor
        let retained = ctx
            .fee_bases
            .and_then(|lookup| lookup.taker_fee_basis(trade.id, &venue_order_id));

        let basis = if let Some(basis) = retained {
            basis
        } else {
            TakerFeeBasis {
                rate: instrument_taker_fee(instrument)?,
                exponent: instrument_fee_exponent(instrument)?,
            }
        };

        let (last_px, commission) = if matches.is_empty() {
            let commission = compute_commission(
                basis.rate,
                basis.exponent,
                trade.taker_size,
                trade.taker_price,
                LiquiditySide::Taker,
            )
            .with_context(|| format!("taker trade {} commission overflow", trade.id))?;
            (last_px, commission)
        } else {
            aggregate_taker_matches(trade, instrument, matches, basis)?
        };

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
            Some(basis),
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
        taker_fee_basis,
        ts_event: trade.ts_event.unwrap_or_default(),
    })
}

// Maker prices describe execution; the top-level taker price can be a limit or one book level
fn validate_taker_matches(
    trade: &NormalizedTrade<'_>,
    instrument: &InstrumentAny,
    ctx: &AdmissionContext<'_>,
) -> Result<Vec<(Decimal, Decimal)>, AdmissionError> {
    let mut matches = Vec::with_capacity(trade.maker_orders.len());
    let mut order_ids = Vec::with_capacity(trade.maker_orders.len());
    let mut complementary_binding = None;

    for maker in trade.maker_orders {
        let maker_instrument = ctx.instruments.get_cloned(&maker.asset_id);
        let validate = || -> anyhow::Result<(Decimal, Decimal)> {
            // Outcome filters can leave only the traded token loaded
            if let Some(maker_instrument) = &maker_instrument {
                validate_instrument_binding(maker_instrument, trade.market, maker.outcome)?;
            }

            check_valid_string_ascii(maker.asset_id.as_str(), "taker counterparty asset")?;
            anyhow::ensure!(
                !maker.outcome.as_str().trim().is_empty(),
                "taker counterparty outcome must not be empty",
            );
            checked_venue_order_id(&maker.order_id, "taker counterparty")?;
            anyhow::ensure!(
                !order_ids.contains(&maker.order_id.as_str()),
                "maker order {} appears more than once in trade {}",
                maker.order_id,
                trade.id,
            );
            let same_asset = maker.asset_id == trade.asset_id;
            anyhow::ensure!(
                same_asset == (maker.outcome == trade.outcome),
                "maker order {} outcome is inconsistent with the taker asset in trade {}",
                maker.order_id,
                trade.id,
            );

            if !same_asset && let Some(binding) = complementary_binding {
                anyhow::ensure!(
                    binding == (maker.asset_id, maker.outcome),
                    "inconsistent complementary token evidence in trade {}",
                    trade.id,
                );
            }

            let side = determine_order_side(
                PolymarketLiquiditySide::Maker,
                trade.side,
                trade.asset_id.as_str(),
                maker.asset_id.as_str(),
            );

            if let Some(provider_side) = maker.side {
                anyhow::ensure!(
                    OrderSide::from(provider_side) == side,
                    "provider maker order {} side {provider_side} contradicts derived side {side}",
                    maker.order_id,
                );
            }

            validate_quantity_evidence(
                maker.matched_amount,
                instrument.size_precision(),
                "taker counterparty matched amount",
                false,
            )?;
            // Counterparty executions are historical evidence on both transports
            let price_precision = maker_instrument
                .as_ref()
                .unwrap_or(instrument)
                .price_precision();
            validate_historical_price_evidence(
                snap_settlement_price(maker.price, maker.matched_amount, price_precision),
                "taker counterparty price",
            )?;
            let price = if same_asset {
                maker.price
            } else {
                Decimal::ONE - maker.price
            };
            Ok((maker.matched_amount, price))
        };
        matches.push(validate().map_err(AdmissionError::Invalid)?);

        if maker.asset_id != trade.asset_id {
            complementary_binding = Some((maker.asset_id, maker.outcome));
        }
        order_ids.push(maker.order_id.as_str());
    }

    if !matches.is_empty() {
        let size = matches
            .iter()
            .try_fold(Decimal::ZERO, |total, (size, _)| {
                total
                    .checked_add(*size)
                    .context("taker matched quantity overflow")
            })
            .map_err(AdmissionError::Invalid)?;
        if size != trade.taker_size {
            return Err(AdmissionError::Invalid(anyhow::anyhow!(
                "maker matched quantity {size} does not equal taker size {} in trade {}",
                trade.taker_size,
                trade.id,
            )));
        }
    }

    Ok(matches)
}

fn aggregate_taker_matches(
    trade: &NormalizedTrade<'_>,
    instrument: &InstrumentAny,
    matches: &[(Decimal, Decimal)],
    basis: TakerFeeBasis,
) -> anyhow::Result<(Price, Decimal)> {
    let mut notional = Decimal::ZERO;
    let mut commission = Decimal::ZERO;

    for &(size, price) in matches {
        notional = size
            .checked_mul(price)
            .and_then(|value| notional.checked_add(value))
            .context("taker matched notional overflow")?;
        let fee = compute_commission_unrounded(
            basis.rate,
            basis.exponent,
            size,
            price,
            LiquiditySide::Taker,
        )?;
        commission = commission
            .checked_add(fee)
            .context("taker matched commission overflow")?;
    }

    let average = notional
        .checked_div(trade.taker_size)
        .context("taker matched average overflow")?;
    let commission = commission.round_dp_with_strategy(5, RoundingStrategy::ToZero);
    let settlement_notional = notional.round_dp(USDC_DECIMALS);
    let preserves_notional = |value: Decimal| -> anyhow::Result<bool> {
        let represented = value
            .checked_mul(trade.taker_size)
            .context("taker represented notional overflow")?;
        Ok(represented.round_dp(USDC_DECIMALS) == settlement_notional)
    };

    if matches.iter().all(|(_, price)| *price == average)
        && let Ok(price) = validate_price(
            trade,
            average,
            trade.taker_size,
            instrument,
            "taker matched price",
        )
        && preserves_notional(price.as_decimal())?
    {
        return Ok((price, commission));
    }

    let rounded = average.round_dp(u32::from(FIXED_PRECISION));
    let increment = Decimal::new(1, u32::from(FIXED_PRECISION));

    // At a cash rounding boundary, the nearest average can land on the wrong side,
    // check adjacent representable prices before rejecting the fill.
    for value in [rounded, rounded - increment, rounded + increment] {
        if value > Decimal::ZERO && value < Decimal::ONE && preserves_notional(value)? {
            let price =
                validate_historical_price_evidence(value.normalize(), "taker matched average")?;
            return Ok((price, commission));
        }
    }

    anyhow::bail!(
        "taker matched average cannot preserve the settlement notional for trade {}",
        trade.id,
    )
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
            maker_order.matched_amount,
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
        taker_fee_basis: None,
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
    size: Decimal,
    instrument: &InstrumentAny,
    field: &str,
) -> anyhow::Result<Price> {
    let value = snap_settlement_price(value, size, instrument.price_precision());

    if trade.from_rest {
        return validate_historical_price_evidence(value, field);
    }

    validate_price_evidence(value, instrument.price_precision(), field)?;
    Price::from_decimal_dp(value, instrument.price_precision())
        .with_context(|| format!("{field} {value} overflow"))
}

// Some venue fill prices are ratios of 6-decimal settlement amounts (0.42 as 0.4200001023529231)
fn snap_settlement_price(value: Decimal, size: Decimal, precision: u8) -> Decimal {
    let snapped = value.round_dp(u32::from(precision));
    if snapped == value || snapped <= Decimal::ZERO || snapped >= Decimal::ONE {
        return value;
    }

    match (value - snapped).checked_mul(size) {
        Some(diff) if diff.abs() <= Decimal::new(1, USDC_DECIMALS) => snapped,
        _ => value,
    }
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

#[cfg(test)]
mod tests {
    use nautilus_core::Params;
    use nautilus_model::{enums::AssetClass, identifiers::Symbol, instruments::BinaryOption};
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;
    use crate::filters::{InstrumentFilter, PredicateFilter};

    fn instruments() -> AtomicMap<Ustr, InstrumentAny> {
        let instruments = AtomicMap::new();

        for (token, outcome) in [("token-no", "No"), ("token-yes", "Yes")] {
            let info: Params = serde_json::from_value(json!({
                "condition_id": "condition",
                "fee_schedule": {"rate": "0.05", "exponent": "1"},
            }))
            .unwrap();
            let binary = BinaryOption::builder()
                .instrument_id(InstrumentId::from(format!("{token}.POLYMARKET").as_str()))
                .raw_symbol(Symbol::from(token))
                .asset_class(AssetClass::Alternative)
                .currency(Currency::pUSD())
                .activation_ns(UnixNanos::default())
                .expiration_ns(UnixNanos::default())
                .price_precision(4)
                .size_precision(6)
                .price_increment(Price::from("0.0010"))
                .size_increment(Quantity::from("0.000001"))
                .outcome(Ustr::from(outcome))
                .info(info)
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap();
            instruments.insert(Ustr::from(token), InstrumentAny::BinaryOption(binary));
        }

        instruments
    }

    fn context(instruments: &AtomicMap<Ustr, InstrumentAny>) -> AdmissionContext<'_> {
        AdmissionContext {
            signer_type: PolymarketSignerType::Owner,
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            instruments,
            fee_bases: None,
        }
    }

    fn multilevel_trade(complement: bool, sell: bool) -> PolymarketUserTrade {
        let mut trade: PolymarketUserTrade =
            serde_json::from_str(include_str!("../../../test_data/ws_user_trade_full.json"))
                .unwrap();
        trade.asset_id = Ustr::from("token-no");
        trade.market = Ustr::from("condition");
        trade.outcome = PolymarketOutcome::no();
        trade.size = "40".to_string();
        trade.price = "0.992".to_string();
        trade.side = if sell {
            PolymarketOrderSide::Sell
        } else {
            PolymarketOrderSide::Buy
        };
        let template = trade.maker_orders[0].clone();
        trade.maker_orders = [("35", dec!(0.992)), ("5", dec!(0.994))]
            .into_iter()
            .enumerate()
            .map(|(index, (size, price))| {
                let mut maker = template.clone();
                maker.order_id = format!("maker-{index}");
                maker.asset_id = Ustr::from(if complement { "token-yes" } else { "token-no" });
                maker.outcome = if complement {
                    PolymarketOutcome::yes()
                } else {
                    PolymarketOutcome::no()
                };
                maker.matched_amount = size.parse().unwrap();
                maker.price = if complement {
                    Decimal::ONE - price
                } else {
                    price
                };
                maker.side = Some(if complement == sell {
                    PolymarketOrderSide::Sell
                } else {
                    PolymarketOrderSide::Buy
                });
                maker
            })
            .collect();
        trade
    }

    #[rstest]
    fn taker_multilevel_economics(
        #[values(false, true)] rest: bool,
        #[values(false, true)] complement: bool,
        #[values(false, true)] sell: bool,
    ) {
        let instruments = instruments();
        let ctx = context(&instruments);
        let trade = multilevel_trade(complement, sell);
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &ctx).unwrap();

        assert_eq!(admitted.legs.len(), 1);
        let leg = &admitted.legs[0];
        assert_eq!(leg.last_qty.as_decimal(), dec!(40));
        assert_eq!(leg.last_px.as_decimal(), dec!(0.99225));
        assert_eq!(
            leg.last_qty.as_decimal() * leg.last_px.as_decimal(),
            dec!(39.690)
        );
        assert_eq!(leg.commission.as_decimal(), dec!(0.01537));
        assert_eq!(leg.trade_id, TradeId::from(trade.id.as_str()));
        assert_eq!(
            leg.order_side,
            if sell {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            }
        );
    }

    #[rstest]
    fn taker_counterparty_prices_preserve_historical_precision(
        #[values(false, true)] rest: bool,
        #[values(false, true)] complement: bool,
        #[values(false, true)] sell: bool,
    ) {
        let instruments = instruments();
        let mut trade = multilevel_trade(complement, sell);
        trade.size = "2".to_string();
        trade.price = "0.42".to_string();

        for (maker, price) in trade
            .maker_orders
            .iter_mut()
            .zip([dec!(0.42), dec!(0.4200014)])
        {
            maker.matched_amount = Decimal::ONE;
            maker.price = if complement {
                Decimal::ONE - price
            } else {
                price
            };
        }

        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        assert_eq!(admitted.legs.len(), 1);
        let leg = &admitted.legs[0];
        assert_eq!(leg.last_qty.as_decimal(), dec!(2));
        assert_eq!(leg.last_px.as_decimal(), dec!(0.4200007));
        assert_eq!(
            (leg.last_qty.as_decimal() * leg.last_px.as_decimal()).round_dp(6),
            dec!(0.840001),
        );
        assert_eq!(leg.commission.as_decimal(), dec!(0.02436));
        assert_eq!(leg.trade_id, TradeId::from(trade.id.as_str()));
        assert_eq!(
            leg.order_side,
            if sell {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            }
        );
    }

    #[rstest]
    fn taker_counterparty_settlement_rounding_remains_supported(#[values(false, true)] rest: bool) {
        let instruments = instruments();
        let mut trade = multilevel_trade(false, false);
        trade.size = "2".to_string();
        trade.price = "0.42".to_string();

        for maker in &mut trade.maker_orders {
            maker.matched_amount = Decimal::ONE;
            maker.price = dec!(0.42);
        }

        trade.maker_orders[1].price += Decimal::new(1, u32::from(FIXED_PRECISION) + 1);
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        assert_eq!(admitted.legs[0].last_px.as_decimal(), dec!(0.42));
        assert_eq!(admitted.legs[0].last_qty.as_decimal(), dec!(2));
        assert_eq!(admitted.legs[0].commission.as_decimal(), dec!(0.02436));
    }

    #[rstest]
    #[case::zero(Decimal::ZERO, "must be greater than zero and less than one")]
    #[case::one(Decimal::ONE, "must be greater than zero and less than one")]
    #[case::beyond_fixed_precision(
        dec!(0.4200014) + Decimal::new(1, u32::from(FIXED_PRECISION) + 1),
        "exceeds the maximum representable price precision",
    )]
    fn taker_rejects_invalid_historical_counterparty_price(
        #[case] price: Decimal,
        #[case] message: &str,
        #[values(false, true)] rest: bool,
    ) {
        let instruments = instruments();
        let mut trade = multilevel_trade(false, false);
        trade.size = "2".to_string();
        trade.price = "0.42".to_string();

        for maker in &mut trade.maker_orders {
            maker.matched_amount = Decimal::ONE;
            maker.price = dec!(0.42);
        }

        trade.maker_orders[1].price = price;
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let error = admit_trade_evidence(evidence, &context(&instruments)).unwrap_err();

        assert!(error.to_string().contains(message), "{error}");
    }

    #[rstest]
    fn taker_multilevel_fee_uses_each_execution_price() {
        let instruments = instruments();
        let mut trade = multilevel_trade(false, false);
        trade.price = "0.1".to_string();
        trade.maker_orders[0].matched_amount = dec!(30);
        trade.maker_orders[0].price = dec!(0.1);
        trade.maker_orders[1].matched_amount = dec!(10);
        trade.maker_orders[1].price = dec!(0.8);

        let admitted =
            admit_trade_evidence(TradeEvidence::Stream(&trade), &context(&instruments)).unwrap();

        assert_eq!(admitted.legs[0].last_px.as_decimal(), dec!(0.275));
        // 30 * 0.05 * 0.1 * 0.9 + 10 * 0.05 * 0.8 * 0.2
        assert_eq!(admitted.legs[0].commission.as_decimal(), dec!(0.215));
    }

    #[rstest]
    fn taker_repeating_average_preserves_cash_precision(#[values(false, true)] rest: bool) {
        let instruments = instruments();
        let mut trade = multilevel_trade(true, false);
        trade.size = "45.030125".to_string();
        trade.price = "0.991".to_string();
        trade.maker_orders[0].matched_amount = dec!(29.99);
        trade.maker_orders[0].price = dec!(0.009);
        trade.maker_orders[1].matched_amount = dec!(15.040125);
        trade.maker_orders[1].price = dec!(0.008);
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        let leg = &admitted.legs[0];
        let notional = dec!(29.99) * dec!(0.991) + dec!(15.040125) * dec!(0.992);
        assert_eq!(leg.last_qty.as_decimal(), dec!(45.030125));
        assert_eq!(
            (leg.last_px.as_decimal() * leg.last_qty.as_decimal()).round_dp(6),
            notional.round_dp(6)
        );
        assert!(leg.last_px.precision > 4);
    }

    #[rstest]
    #[case::upper_cash_midpoint(dec!(1.999985), dec!(1.000015), dec!(1.000002))]
    #[case::lower_cash_midpoint(dec!(1.999975), dec!(1.000025), dec!(1.000002))]
    fn taker_average_preserves_cash_at_rounding_midpoint(
        #[case] first_size: Decimal,
        #[case] second_size: Decimal,
        #[case] expected_cash: Decimal,
        #[values(false, true)] rest: bool,
    ) {
        let instruments = instruments();
        let mut trade = multilevel_trade(false, false);
        trade.size = "3".to_string();
        trade.price = "0.3".to_string();
        trade.maker_orders[0].matched_amount = first_size;
        trade.maker_orders[0].price = dec!(0.3);
        trade.maker_orders[1].matched_amount = second_size;
        trade.maker_orders[1].price = dec!(0.4);
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        let leg = &admitted.legs[0];
        assert_eq!(leg.last_qty.as_decimal(), dec!(3));
        assert_eq!(
            (leg.last_px.as_decimal() * leg.last_qty.as_decimal()).round_dp(6),
            expected_cash,
        );
    }

    #[rstest]
    fn taker_rejects_notional_without_a_representable_average(#[values(false, true)] rest: bool) {
        let instruments = instruments();
        let mut trade = multilevel_trade(false, false);

        // One price increment changes cash by 0.0001, so a 0.000001 offset cannot be represented
        let size = Decimal::from(10_u64.pow(u32::from(FIXED_PRECISION) - 4));
        trade.size = size.to_string();
        trade.price = "0.3".to_string();
        trade.maker_orders[0].matched_amount = size / dec!(2) - dec!(0.00001);
        trade.maker_orders[0].price = dec!(0.3);
        trade.maker_orders[1].matched_amount = size / dec!(2) + dec!(0.00001);
        trade.maker_orders[1].price = dec!(0.4);
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let error = admit_trade_evidence(evidence, &context(&instruments)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cannot preserve the settlement notional")
        );
    }

    #[rstest]
    #[case::short_quantity("quantity")]
    #[case::excess_quantity("excess")]
    #[case::duplicate_order("duplicate")]
    #[case::wrong_side("side")]
    #[case::wrong_outcome("outcome")]
    #[case::zero_price("price")]
    #[case::negative_quantity("negative")]
    fn taker_rejects_inconsistent_counterparty_evidence(#[case] corruption: &str) {
        let instruments = instruments();
        let mut trade = multilevel_trade(true, false);

        match corruption {
            "quantity" => trade.maker_orders[0].matched_amount = dec!(34),
            "excess" => trade.maker_orders[0].matched_amount = dec!(36),
            "duplicate" => trade.maker_orders[1].order_id = trade.maker_orders[0].order_id.clone(),
            "side" => trade.maker_orders[0].side = Some(PolymarketOrderSide::Sell),
            "outcome" => trade.maker_orders[0].outcome = PolymarketOutcome::no(),
            "price" => trade.maker_orders[0].price = Decimal::ZERO,
            "negative" => trade.maker_orders[0].matched_amount = -Decimal::ONE,
            _ => unreachable!(),
        }

        let result = admit_trade_evidence(TradeEvidence::Stream(&trade), &context(&instruments));

        assert!(
            matches!(result, Err(AdmissionError::Invalid(_))),
            "{result:?}"
        );
    }

    #[rstest]
    fn taker_admits_complementary_matches_with_filtered_instruments(
        #[values(false, true)] rest: bool,
        #[values(false, true)] sell: bool,
        #[values(false, true)] yes: bool,
    ) {
        let available = instruments();
        let instruments = AtomicMap::new();
        let mut trade = multilevel_trade(true, sell);
        if yes {
            trade.asset_id = Ustr::from("token-yes");
            trade.outcome = PolymarketOutcome::yes();
            for maker in &mut trade.maker_orders {
                maker.asset_id = Ustr::from("token-no");
                maker.outcome = PolymarketOutcome::no();
            }
        }

        let filter = PredicateFilter::outcome(trade.outcome.as_str());

        for token in ["token-no", "token-yes"] {
            let token = Ustr::from(token);
            let instrument = available.get_cloned(&token).unwrap();
            if filter.accept(&instrument) {
                instruments.insert(token, instrument);
            }
        }

        assert!(instruments.get_cloned(&trade.asset_id).is_some());
        assert!(
            instruments
                .get_cloned(&trade.maker_orders[0].asset_id)
                .is_none()
        );
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        assert_eq!(admitted.legs.len(), 1);
        let leg = &admitted.legs[0];
        assert_eq!(leg.last_qty.as_decimal(), dec!(40));
        assert_eq!(leg.last_px.as_decimal(), dec!(0.99225));
        assert_eq!(leg.commission.as_decimal(), dec!(0.01537));
        assert_eq!(leg.trade_id, TradeId::from(trade.id.as_str()));
        assert_eq!(
            leg.order_side,
            if sell {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            },
        );
    }

    #[rstest]
    #[case::empty_asset("asset")]
    #[case::empty_outcome("outcome")]
    #[case::third_token("third_token")]
    #[case::conflicting_outcome("conflicting_outcome")]
    fn taker_rejects_inconsistent_unloaded_counterparty_evidence(
        #[case] corruption: &str,
        #[values(false, true)] rest: bool,
    ) {
        let available = instruments();
        let instruments = AtomicMap::new();
        let mut trade = multilevel_trade(true, false);
        instruments.insert(
            trade.asset_id,
            available.get_cloned(&trade.asset_id).unwrap(),
        );

        match corruption {
            "asset" => trade.maker_orders[0].asset_id = Ustr::from(""),
            "outcome" => trade.maker_orders[0].outcome = PolymarketOutcome::from(""),
            "third_token" => trade.maker_orders[1].asset_id = Ustr::from("third-token"),
            "conflicting_outcome" => {
                trade.maker_orders[1].outcome = PolymarketOutcome::from("other-outcome");
            }
            _ => unreachable!(),
        }
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let result = admit_trade_evidence(evidence, &context(&instruments));

        assert!(
            matches!(result, Err(AdmissionError::Invalid(_))),
            "{result:?}"
        );
    }

    #[rstest]
    fn taker_rejects_counterparty_from_another_condition() {
        let instruments = instruments();
        let mut instrument = instruments.get_cloned(&Ustr::from("token-yes")).unwrap();
        let InstrumentAny::BinaryOption(binary) = &mut instrument else {
            unreachable!();
        };
        binary.info =
            Some(serde_json::from_value(json!({"condition_id": "other-condition"})).unwrap());
        instruments.insert(Ustr::from("token-yes"), instrument);
        let trade = multilevel_trade(true, false);

        let error = admit_trade_evidence(TradeEvidence::Stream(&trade), &context(&instruments))
            .unwrap_err();

        assert!(error.to_string().contains("condition"));
    }

    #[rstest]
    fn taker_preserves_top_level_fallback_without_maker_evidence() {
        let instruments = instruments();
        let mut trade = multilevel_trade(true, false);
        trade.maker_orders.clear();

        let admitted =
            admit_trade_evidence(TradeEvidence::Stream(&trade), &context(&instruments)).unwrap();

        assert_eq!(admitted.legs[0].last_px.as_decimal(), dec!(0.992));
        assert_eq!(admitted.legs[0].commission.as_decimal(), dec!(0.01587));
    }

    #[rstest]
    fn taker_counterparty_side_can_be_derived_when_omitted(#[values(false, true)] rest: bool) {
        let instruments = instruments();
        let mut trade = multilevel_trade(true, false);

        for maker in &mut trade.maker_orders {
            maker.side = None;
        }

        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        assert_eq!(admitted.legs[0].last_px.as_decimal(), dec!(0.99225));
    }

    #[rstest]
    fn taker_average_does_not_snap_across_a_cash_rounding_boundary() {
        let instruments = instruments();
        let trade = multilevel_trade(false, false);
        let mut report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        report.size = dec!(1);
        report.price = dec!(0.42);
        report.maker_orders[0].matched_amount = dec!(0.5);
        report.maker_orders[0].price = dec!(0.42);
        report.maker_orders[1].matched_amount = dec!(0.5);
        report.maker_orders[1].price = dec!(0.4200014);

        let admitted =
            admit_trade_evidence(TradeEvidence::Rest(&report), &context(&instruments)).unwrap();

        assert_eq!(admitted.legs[0].last_px.as_decimal(), dec!(0.4200007));
    }

    #[rstest]
    #[case::rest(true, "0.4200014")]
    #[case::stream(false, "0.42")]
    fn taker_equal_price_preserves_cash_precision(#[case] rest: bool, #[case] top_price: &str) {
        let instruments = instruments();
        let mut trade = multilevel_trade(false, false);
        trade.size = "1".to_string();
        trade.price = top_price.to_string();
        trade.maker_orders.truncate(1);
        trade.maker_orders[0].matched_amount = dec!(1);
        trade.maker_orders[0].price = dec!(0.4200007);
        let report: PolymarketTradeReport =
            serde_json::from_value(serde_json::to_value(&trade).unwrap()).unwrap();
        let evidence = if rest {
            TradeEvidence::Rest(&report)
        } else {
            TradeEvidence::Stream(&trade)
        };

        let admitted = admit_trade_evidence(evidence, &context(&instruments)).unwrap();

        let leg = &admitted.legs[0];
        assert_eq!(leg.last_px.as_decimal(), dec!(0.4200007));
        assert_eq!(
            (leg.last_px.as_decimal() * leg.last_qty.as_decimal()).round_dp(6),
            dec!(0.420001),
        );
    }
}

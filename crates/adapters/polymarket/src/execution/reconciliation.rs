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

//! Reconciliation report generation for the Polymarket execution client.

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_common::cache::Cache;
use nautilus_core::{
    DurationNanos, UnixNanos, collections::AtomicMap, datetime::NANOSECONDS_IN_SECOND,
    time::AtomicTime,
};
use nautilus_model::{
    enums::{InstrumentCloseType, OrderSide, OrderStatus, PositionSide, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    order_fill_tracker::OrderFillTrackerMap,
    parse::{OrderReportParseContext, parse_timestamp, parse_validated_order_status_report},
    settlement::{AdmissionContext, AdmissionError, AdmittedLeg, TradeEvidence, admit_trade_legs},
};
use crate::{
    common::{
        consts::{DUST_POSITION_THRESHOLD, DUST_SNAP_THRESHOLD_DEC, USDC_DECIMALS},
        enums::{
            PolymarketLiquiditySide, PolymarketOutcome, PolymarketSignerType, PolymarketTradeStatus,
        },
        models::is_owned_by_account,
    },
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        models::{DataApiPosition, PolymarketOpenOrder, PolymarketTradeReport},
        query::{GetOrdersParams, GetTradesParams},
    },
};

pub(crate) fn venue_leg_filled_before_and_quantity(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    size_precision: u8,
) -> anyhow::Result<(Quantity, Quantity)> {
    let mut filled = Decimal::ZERO;

    for event in order.events() {
        match event {
            OrderEventAny::Filled(event) if event.venue_order_id == venue_order_id => {
                filled += event.last_qty.as_decimal();
            }
            OrderEventAny::FillVoided(event) if event.venue_order_id == venue_order_id => {
                filled -= event.voided_qty.as_decimal();
            }
            _ => {}
        }
    }

    anyhow::ensure!(filled >= Decimal::ZERO, "venue-leg fills are negative");
    let current_leg_filled = Quantity::from_decimal_dp(filled, size_precision)
        .context("venue-leg fills exceed quantity precision")?;
    anyhow::ensure!(
        current_leg_filled.as_decimal() == filled,
        "venue-leg fills cannot be represented exactly"
    );
    let filled_before = order
        .filled_qty()
        .checked_sub(current_leg_filled)
        .context("current venue-leg fills exceed cumulative fills")?;
    let leg_quantity = order
        .quantity()
        .checked_sub(filled_before)
        .context("fills before current venue leg exceed logical quantity")?;
    Ok((filled_before, leg_quantity))
}

/// Shared context for trade-to-fill-report conversion.
pub(crate) struct FillContext<'a> {
    pub account_id: AccountId,
    pub signer_type: PolymarketSignerType,
    pub user_address: &'a str,
    pub api_key: &'a str,
    pub pusd: Currency,
    pub clock: &'static AtomicTime,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FillReportScope {
    instrument_id: Option<InstrumentId>,
    venue_order_id: Option<VenueOrderId>,
    expected_order_side: Option<OrderSide>,
}

impl FillReportScope {
    pub(crate) const fn new(
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> Self {
        Self {
            instrument_id,
            venue_order_id,
            expected_order_side: None,
        }
    }

    pub(crate) const fn with_expected_order_side(
        mut self,
        expected_order_side: Option<OrderSide>,
    ) -> Self {
        self.expected_order_side = if self.venue_order_id.is_some() {
            expected_order_side
        } else {
            None
        };
        self
    }
}

fn validate_expected_order_side(
    expected: Option<OrderSide>,
    actual: OrderSide,
    evidence: &str,
) -> anyhow::Result<()> {
    if let Some(expected) = expected {
        anyhow::ensure!(
            actual == expected,
            "{evidence} side {actual} does not match known order side {expected}",
        );
    }
    Ok(())
}

fn validate_target_trade_role(
    trade: &PolymarketTradeReport,
    venue_order_id: VenueOrderId,
) -> anyhow::Result<bool> {
    let target_is_taker = trade.taker_order_id == venue_order_id.as_str();
    let target_maker_occurrences = trade
        .maker_orders
        .iter()
        .filter(|order| order.order_id == venue_order_id.as_str())
        .count();
    let target_is_maker = target_maker_occurrences > 0;
    if !target_is_taker && !target_is_maker {
        return Ok(false);
    }
    anyhow::ensure!(
        usize::from(target_is_taker) + target_maker_occurrences == 1,
        "target order {venue_order_id} appears more than once in trade {}",
        trade.id,
    );
    let declared_maker = trade.trader_side == PolymarketLiquiditySide::Maker;
    anyhow::ensure!(
        declared_maker == target_is_maker,
        "trade {} trader_side {:?} contradicts target order {venue_order_id} participant role",
        trade.id,
        trade.trader_side,
    );
    Ok(true)
}

pub(crate) fn checked_venue_order_id(value: &str, evidence: &str) -> anyhow::Result<VenueOrderId> {
    VenueOrderId::new_checked(value)
        .with_context(|| format!("{evidence} has invalid venue order ID {value:?}"))
}

pub(crate) fn validate_instrument_binding(
    instrument: &InstrumentAny,
    condition_id: &str,
    outcome: PolymarketOutcome,
) -> anyhow::Result<()> {
    let InstrumentAny::BinaryOption(binary) = instrument else {
        anyhow::bail!("expected Polymarket BinaryOption instrument, found {instrument:?}");
    };
    let instrument_condition = binary
        .info
        .as_ref()
        .and_then(|info| info.get_str("condition_id"))
        .context("Polymarket instrument is missing condition_id metadata")?;

    anyhow::ensure!(
        instrument_condition.eq_ignore_ascii_case(condition_id),
        "provider condition {condition_id} does not match instrument condition {instrument_condition}",
    );
    let instrument_outcome = binary
        .outcome
        .context("Polymarket instrument is missing outcome metadata")?;
    anyhow::ensure!(
        instrument_outcome == outcome.as_str(),
        "provider outcome {outcome} does not match instrument outcome {instrument_outcome}",
    );

    Ok(())
}

pub(crate) fn validate_quantity_evidence(
    value: Decimal,
    precision: u8,
    field: &str,
    allow_zero: bool,
) -> anyhow::Result<()> {
    if allow_zero {
        anyhow::ensure!(
            value >= Decimal::ZERO,
            "{field} {value} must be non-negative"
        );
    } else {
        anyhow::ensure!(value > Decimal::ZERO, "{field} {value} must be positive");
    }

    let quantity = Quantity::from_decimal_dp(value, precision).with_context(|| {
        format!("failed to represent {field} {value} with quantity precision {precision}")
    })?;
    anyhow::ensure!(
        quantity.as_decimal() == value,
        "{field} {value} is not exactly representable with quantity precision {precision}",
    );
    Ok(())
}

pub(crate) fn validate_price_evidence(
    value: Decimal,
    precision: u8,
    field: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        value > Decimal::ZERO && value < Decimal::ONE,
        "{field} {value} must be greater than zero and less than one",
    );
    let price = Price::from_decimal_dp(value, precision).with_context(|| {
        format!("failed to represent {field} {value} with price precision {precision}")
    })?;
    anyhow::ensure!(
        price.as_decimal() == value,
        "{field} {value} is not exactly representable with price precision {precision}",
    );
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ValidatedOrderRow {
    venue_order_id: VenueOrderId,
    ts_accepted: UnixNanos,
    expire_time: Option<UnixNanos>,
}

fn parse_provider_order_expiration(
    order: &PolymarketOpenOrder,
) -> anyhow::Result<Option<UnixNanos>> {
    match order.expiration.as_deref() {
        None => Ok(None),
        Some(value) => {
            let seconds: u64 = value.parse().with_context(|| {
                format!("provider order {} has invalid expiration {value}", order.id)
            })?;

            if seconds == 0 {
                return Ok(None);
            }
            seconds
                .checked_mul(NANOSECONDS_IN_SECOND)
                .map(UnixNanos::from)
                .map(Some)
                .with_context(|| {
                    format!("provider order {} has invalid expiration {value}", order.id)
                })
        }
    }
}

fn validate_order_row_values(
    order: &PolymarketOpenOrder,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<ValidatedOrderRow> {
    validate_quantity_evidence(
        order.original_size,
        size_precision,
        &format!("provider order {} quantity", order.id),
        false,
    )?;
    validate_quantity_evidence(
        order.size_matched,
        size_precision,
        &format!("provider order {} matched quantity", order.id),
        true,
    )?;
    validate_price_evidence(
        order.price,
        price_precision,
        &format!("provider order {} price", order.id),
    )?;
    let ts_accepted = order
        .created_at
        .checked_mul(NANOSECONDS_IN_SECOND)
        .with_context(|| {
            format!(
                "provider order {} created_at seconds {} overflow Unix nanoseconds",
                order.id, order.created_at,
            )
        })?;
    let expire_time = parse_provider_order_expiration(order)?;
    anyhow::ensure!(
        TimeInForce::from(order.order_type) != TimeInForce::Gtd || expire_time.is_some(),
        "provider GTD order {} requires a valid positive expiration",
        order.id,
    );
    Ok(ValidatedOrderRow {
        venue_order_id: checked_venue_order_id(&order.id, "provider order")?,
        ts_accepted: UnixNanos::from(ts_accepted),
        expire_time,
    })
}

fn resolve_target_instrument(
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    token_id: Ustr,
    requested_instrument_id: Option<InstrumentId>,
    evidence: &str,
) -> anyhow::Result<InstrumentAny> {
    let instrument = instruments.get_cloned(&token_id).with_context(|| {
        requested_instrument_id.map_or_else(
            || format!("{evidence} token {token_id} has no loaded Polymarket instrument"),
            |requested_instrument_id| {
                format!(
                    "{evidence} token {token_id} has no loaded Polymarket instrument for requested instrument {requested_instrument_id}"
                )
            },
        )
    })?;

    if let Some(requested_instrument_id) = requested_instrument_id {
        anyhow::ensure!(
            instrument.id() == requested_instrument_id,
            "{evidence} resolves to instrument {}, not requested instrument {requested_instrument_id}",
            instrument.id(),
        );
    }
    Ok(instrument)
}

pub(super) fn validate_client_bound_order_quantity(
    provider_order: &PolymarketOpenOrder,
    expected_quantity: Quantity,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        expected_quantity.as_decimal() == provider_order.original_size,
        "provider order quantity {} does not match cached order quantity {}",
        provider_order.original_size,
        expected_quantity,
    );
    Ok(())
}

fn validate_client_bound_order_row(
    provider_order: &PolymarketOpenOrder,
    cached_order: &OrderAny,
    expected_quantity: Quantity,
    provider_expire_time: Option<UnixNanos>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        cached_order.order_side() == provider_order.side.into(),
        "provider order side {} does not match cached order side {}",
        provider_order.side,
        cached_order.order_side(),
    );
    anyhow::ensure!(
        cached_order.time_in_force() == provider_order.order_type.into(),
        "provider order time in force {} does not match cached order time in force {}",
        provider_order.order_type,
        cached_order.time_in_force(),
    );
    validate_client_bound_order_quantity(provider_order, expected_quantity)?;
    let cached_price = cached_order
        .price()
        .context("cached Limit order is missing price")?;
    anyhow::ensure!(
        cached_price.as_decimal() == provider_order.price,
        "provider order price {} does not match cached order price {cached_price}",
        provider_order.price,
    );

    let provider_expire_seconds = provider_expire_time.map(|value| value.as_seconds());
    let cached_expire_seconds = cached_order
        .expire_time()
        .filter(|value| !value.is_zero())
        .map(|value| value.as_seconds());
    if cached_order.time_in_force() == TimeInForce::Gtd {
        anyhow::ensure!(
            cached_expire_seconds == provider_expire_seconds,
            "provider order expiration seconds {provider_expire_seconds:?} do not match cached order expiration seconds {cached_expire_seconds:?}",
        );
    }

    Ok(())
}

struct OrderRowResult {
    report: Option<OrderStatusReport>,
    counted_filtered: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct TargetOrderReportScope<'a> {
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
    cached_order: Option<&'a OrderAny>,
    expected_quantity: Option<Quantity>,
}

impl<'a> TargetOrderReportScope<'a> {
    pub(crate) fn new(
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        cached_order: Option<&'a OrderAny>,
        expected_quantity: Option<Quantity>,
    ) -> Self {
        Self {
            instrument_id,
            venue_order_id,
            client_order_id,
            cached_order,
            expected_quantity,
        }
    }
}

#[derive(Clone, Copy)]
enum OrderEvidenceScope<'a> {
    Collection {
        instrument_filter: Option<InstrumentId>,
    },
    Target {
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        cached_order: Option<&'a OrderAny>,
        expected_quantity: Option<Quantity>,
    },
}

fn build_order_report_from_order(
    order: &PolymarketOpenOrder,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    ctx: &FillContext<'_>,
    scope: OrderEvidenceScope<'_>,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<OrderRowResult> {
    let collection_load_ids = match scope {
        OrderEvidenceScope::Collection {
            instrument_filter: None,
        } => load_ids,
        _ => None,
    };

    if let OrderEvidenceScope::Target { venue_order_id, .. } = scope {
        anyhow::ensure!(
            order.id == venue_order_id.as_str(),
            "provider venue order {} does not match requested venue order {venue_order_id}",
            order.id,
        );
    }

    if !is_owned_by_account(
        &order.maker_address,
        &order.owner,
        ctx.user_address,
        ctx.api_key,
        ctx.signer_type,
    ) {
        return match scope {
            OrderEvidenceScope::Collection { .. } => {
                log::debug!("Dropping open order {} not owned by the account", order.id);
                Ok(OrderRowResult {
                    report: None,
                    counted_filtered: true,
                })
            }
            OrderEvidenceScope::Target { .. } => {
                anyhow::bail!(
                    "provider venue order {} is not owned by the account",
                    order.id
                )
            }
        };
    }

    let instrument = match scope {
        OrderEvidenceScope::Target { instrument_id, .. } => resolve_target_instrument(
            instruments,
            order.asset_id,
            Some(instrument_id),
            &format!("provider venue order {}", order.id),
        )?,
        OrderEvidenceScope::Collection { instrument_filter } => match instruments
            .get_cloned(&order.asset_id)
        {
            Some(instrument) => instrument,
            None => {
                let instrument_id =
                    instrument_id_from_market_token(order.market.as_str(), order.asset_id.as_str());

                if instrument_filter.is_some_and(|filter_id| {
                    !polymarket_instrument_ids_equivalent(filter_id, instrument_id)
                }) {
                    return Ok(OrderRowResult {
                        report: None,
                        counted_filtered: false,
                    });
                }

                if instrument_in_load_ids_scope(instrument_id, collection_load_ids) {
                    anyhow::bail!(unmapped_in_scope_message(
                        "open order",
                        instrument_id,
                        Some(&format!("token {}", order.asset_id)),
                        collection_load_ids,
                    ));
                }
                log::debug!("Dropping out-of-scope unmapped open order instrument {instrument_id}");
                return Ok(OrderRowResult {
                    report: None,
                    counted_filtered: true,
                });
            }
        },
    };
    let instrument_id = instrument.id();

    if let OrderEvidenceScope::Collection {
        instrument_filter: Some(filter_id),
    } = scope
        && !polymarket_instrument_ids_equivalent(filter_id, instrument_id)
    {
        return Ok(OrderRowResult {
            report: None,
            counted_filtered: false,
        });
    }

    if matches!(
        scope,
        OrderEvidenceScope::Collection {
            instrument_filter: None
        }
    ) && !instrument_in_load_ids_scope(instrument_id, collection_load_ids)
    {
        log::debug!("Dropping loaded out-of-scope open order instrument {instrument_id}");
        return Ok(OrderRowResult {
            report: None,
            counted_filtered: true,
        });
    }

    validate_instrument_binding(&instrument, order.market.as_str(), order.outcome)?;
    let validated = validate_order_row_values(
        order,
        instrument.price_precision(),
        instrument.size_precision(),
    )?;
    let (client_order_id, cached_order, expected_quantity) = match scope {
        OrderEvidenceScope::Collection { .. } => (None, None, None),
        OrderEvidenceScope::Target {
            client_order_id,
            cached_order,
            expected_quantity,
            ..
        } => (client_order_id, cached_order, expected_quantity),
    };

    if let Some(cached_order) = cached_order {
        let expected_quantity = expected_quantity.unwrap_or_else(|| cached_order.quantity());
        validate_client_bound_order_row(
            order,
            cached_order,
            expected_quantity,
            validated.expire_time,
        )?;
    }

    let report = parse_validated_order_status_report(
        order,
        OrderReportParseContext {
            instrument_id,
            account_id: ctx.account_id,
            client_order_id,
            venue_order_id: validated.venue_order_id,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
            ts_accepted: validated.ts_accepted,
            expire_time: validated.expire_time,
            ts_init,
        },
    )?;
    Ok(OrderRowResult {
        report: Some(report),
        counted_filtered: false,
    })
}

pub(crate) fn build_target_order_report(
    order: &PolymarketOpenOrder,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    ctx: &FillContext<'_>,
    scope: TargetOrderReportScope<'_>,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let logical_fill_offset = match (scope.cached_order, scope.expected_quantity) {
        (Some(cached_order), None) => {
            let (filled_before, leg_quantity) = venue_leg_filled_before_and_quantity(
                cached_order,
                scope.venue_order_id,
                cached_order.quantity().precision,
            )?;
            Some((cached_order.quantity(), filled_before, leg_quantity))
        }
        _ => None,
    };

    let expected_quantity = scope
        .expected_quantity
        .or_else(|| logical_fill_offset.map(|(_, _, leg_quantity)| leg_quantity));
    let mut report = build_order_report_from_order(
        order,
        instruments,
        ctx,
        OrderEvidenceScope::Target {
            instrument_id: scope.instrument_id,
            venue_order_id: scope.venue_order_id,
            client_order_id: scope.client_order_id,
            cached_order: scope.cached_order,
            expected_quantity,
        },
        ts_init,
        None,
    )?
    .report
    .context("target order evidence was unexpectedly ignored")?;

    if let Some((logical_quantity, filled_before, _)) = logical_fill_offset {
        report.quantity = logical_quantity;
        report.filled_qty = filled_before
            .checked_add(report.filled_qty)
            .context("logical filled quantity overflow")?;
    }

    Ok(report)
}

/// Counts of confirmed trade evidence dropped while building fill reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FillBuildDiscards {
    /// Whether valid unsettled evidence for the requested venue order was found.
    pub has_pending_target: bool,
    /// Fill entries dropped because their instrument is not loaded.
    pub unmapped_instruments: usize,
    /// In-scope historical fills dropped because their instrument is not loaded.
    pub in_scope_historical: usize,
    /// Confirmed maker trades dropped because no maker order in the match is
    /// owned by the account.
    pub unowned_maker_trades: usize,
    /// Confirmed trades dropped from a bounded report because their event time is invalid.
    pub untimestamped_trades: usize,
}

pub(crate) fn admit_selected_trade<'a>(
    selected_trades: &mut AHashMap<&'a str, &'a PolymarketTradeReport>,
    trade: &'a PolymarketTradeReport,
) -> anyhow::Result<bool> {
    if let Some(previous) = selected_trades.get(trade.id.as_str()) {
        anyhow::ensure!(
            *previous == trade,
            "provider trade {} repeats with contradictory evidence",
            trade.id,
        );
        return Ok(false);
    }

    selected_trades.insert(trade.id.as_str(), trade);
    Ok(true)
}

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
///
/// Every reported leg passes the settlement admission boundary shared with WebSocket evidence.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    scope: FillReportScope,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
    lookback_start: Option<UnixNanos>,
) -> anyhow::Result<(Vec<FillReport>, FillBuildDiscards)> {
    let admission_ctx = AdmissionContext {
        signer_type: ctx.signer_type,
        user_address: ctx.user_address,
        api_key: ctx.api_key,
        pusd: ctx.pusd,
        instruments,
    };

    if let Some(target_order_id) = scope.venue_order_id {
        return build_target_fill_reports(
            trades,
            &admission_ctx,
            ctx.account_id,
            scope,
            target_order_id,
            ts_init,
        );
    }

    let mut reports = Vec::new();
    let mut discards = FillBuildDiscards::default();
    let mut selected_trades = AHashMap::new();

    for trade in trades {
        if trade.status != PolymarketTradeStatus::Confirmed {
            continue;
        }

        let selected_order_ids = select_reportable_order_ids(
            trade,
            ctx,
            instruments,
            scope.instrument_id,
            load_ids,
            lookback_start,
            &mut discards,
        );

        if selected_order_ids.is_empty() {
            continue;
        }

        let admitted = match admit_trade_legs(
            TradeEvidence::Rest(trade),
            &admission_ctx,
            &selected_order_ids,
        ) {
            Ok(admitted) => admitted,
            Err(AdmissionError::Untimestamped(e)) => {
                // A bounded report counts the trade as untimestamped; an unbounded one fails
                if trade_in_lookback_window(None, lookback_start, true, &trade.id, &mut discards) {
                    return Err(e);
                }

                continue;
            }
            Err(AdmissionError::Invalid(e)) => return Err(e),
            Err(e) => {
                anyhow::bail!("selected trade {} was not admitted: {e}", trade.id)
            }
        };

        if !trade_in_lookback_window(
            parse_timestamp(&trade.match_time),
            lookback_start,
            true,
            &trade.id,
            &mut discards,
        ) {
            continue;
        }

        if !admit_selected_trade(&mut selected_trades, trade)? {
            continue;
        }

        reports.extend(
            admitted
                .legs
                .iter()
                .map(|leg| leg.fill_report(ctx.account_id, ts_init)),
        );
    }

    Ok((reports, discards))
}

fn build_target_fill_reports(
    trades: &[PolymarketTradeReport],
    ctx: &AdmissionContext<'_>,
    account_id: AccountId,
    scope: FillReportScope,
    target_order_id: VenueOrderId,
    ts_init: UnixNanos,
) -> anyhow::Result<(Vec<FillReport>, FillBuildDiscards)> {
    let mut reports = Vec::new();
    let mut discards = FillBuildDiscards::default();
    let mut selected_trades = AHashMap::new();

    for trade in trades {
        let target = classify_target_trade(
            trade,
            ctx,
            scope.instrument_id,
            target_order_id,
            scope.expected_order_side,
        )?;

        if matches!(target, TargetTrade::Unrelated) {
            continue;
        }

        if !admit_selected_trade(&mut selected_trades, trade)? {
            continue;
        }

        match target {
            TargetTrade::Unrelated | TargetTrade::Failed => {}
            TargetTrade::Pending => discards.has_pending_target = true,
            TargetTrade::Confirmed(leg) => reports.push(leg.fill_report(account_id, ts_init)),
        }
    }

    Ok((reports, discards))
}

enum TargetTrade {
    Unrelated,
    Pending,
    Confirmed(AdmittedLeg),
    Failed,
}

fn classify_target_trade(
    trade: &PolymarketTradeReport,
    ctx: &AdmissionContext<'_>,
    instrument_id: Option<InstrumentId>,
    venue_order_id: VenueOrderId,
    expected_order_side: Option<OrderSide>,
) -> anyhow::Result<TargetTrade> {
    if !validate_target_trade_role(trade, venue_order_id)? {
        return Ok(TargetTrade::Unrelated);
    }

    let admitted = admit_trade_legs(TradeEvidence::Rest(trade), ctx, &[venue_order_id.as_str()])
        .map_err(|e| match e {
            AdmissionError::Untimestamped(e) | AdmissionError::Invalid(e) => e,
            AdmissionError::Unowned => {
                anyhow::anyhow!("target order {venue_order_id} is not owned by the account")
            }
            AdmissionError::UnknownInstrument(token_id) => anyhow::anyhow!(
                "target trade {} token {token_id} has no loaded Polymarket instrument",
                trade.id,
            ),
        })?;

    let status = admitted.status;
    let leg = admitted
        .legs
        .into_iter()
        .next()
        .context("admitted target trade holds no leg")?;

    if let Some(requested_instrument_id) = instrument_id {
        anyhow::ensure!(
            leg.instrument_id == requested_instrument_id,
            "target trade {} resolves to instrument {}, not requested instrument {requested_instrument_id}",
            trade.id,
            leg.instrument_id,
        );
    }

    validate_expected_order_side(
        expected_order_side,
        leg.order_side,
        &format!("target order {venue_order_id}"),
    )?;

    Ok(match status {
        PolymarketTradeStatus::Matched
        | PolymarketTradeStatus::MatchedNotBroadcasted
        | PolymarketTradeStatus::Mined
        | PolymarketTradeStatus::Retrying => TargetTrade::Pending,
        PolymarketTradeStatus::Confirmed => TargetTrade::Confirmed(leg),
        PolymarketTradeStatus::Failed => TargetTrade::Failed,
    })
}

fn select_reportable_order_ids<'a>(
    trade: &'a PolymarketTradeReport,
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    load_ids: Option<&[InstrumentId]>,
    lookback_start: Option<UnixNanos>,
    discards: &mut FillBuildDiscards,
) -> Vec<&'a str> {
    let owned_orders: Vec<(&str, Ustr)> = if trade.trader_side == PolymarketLiquiditySide::Maker {
        trade
            .maker_orders
            .iter()
            .filter(|mo| mo.is_owned_by(ctx.user_address, ctx.api_key, ctx.signer_type))
            .map(|mo| (mo.order_id.as_str(), mo.asset_id))
            .collect()
    } else if is_owned_by_account(
        &trade.maker_address,
        &trade.owner,
        ctx.user_address,
        ctx.api_key,
        ctx.signer_type,
    ) {
        vec![(trade.taker_order_id.as_str(), trade.asset_id)]
    } else {
        log::debug!(
            "Dropping confirmed taker trade {} not owned by the account",
            trade.id
        );
        return Vec::new();
    };

    if owned_orders.is_empty() {
        let instrument_id =
            instrument_id_from_market_token(trade.market.as_str(), trade.asset_id.as_str());

        if trade_in_lookback_window(
            parse_timestamp(&trade.match_time),
            lookback_start,
            instrument_in_load_ids_scope(instrument_id, load_ids),
            &trade.id,
            discards,
        ) {
            discards.unowned_maker_trades += 1;
            log::debug!(
                "Confirmed maker trade {} holds no maker order owned by the account",
                trade.id,
            );
        }

        return Vec::new();
    }

    let mut selected = Vec::new();

    for (order_id, token_id) in owned_orders {
        let Some(instrument) = instruments.get_cloned(&token_id) else {
            classify_unmapped_historical(discards, load_ids, &trade.market, token_id.as_str());
            continue;
        };

        let instrument_id = instrument.id();

        if instrument_filter.is_some_and(|requested| instrument_id != requested) {
            continue;
        }

        if !instrument_in_load_ids_scope(instrument_id, load_ids) {
            log::debug!("Dropping loaded out-of-scope historical instrument {instrument_id}");
            continue;
        }

        selected.push(order_id);
    }

    selected
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    ctx: &FillContext<'_>,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<(Vec<OrderStatusReport>, usize)> {
    let mut reports = Vec::new();
    let mut filtered = 0usize;
    let mut selected_orders = AHashMap::new();

    for order in orders {
        let result = build_order_report_from_order(
            order,
            instruments,
            ctx,
            OrderEvidenceScope::Collection { instrument_filter },
            ts_init,
            load_ids,
        )?;

        if let Some(report) = result.report {
            if let Some(previous) = selected_orders.get(&report.venue_order_id) {
                anyhow::ensure!(
                    *previous == order,
                    "provider venue order {} repeats with contradictory evidence",
                    report.venue_order_id,
                );
                continue;
            }
            selected_orders.insert(report.venue_order_id, order);
            reports.push(report);
        } else {
            filtered += usize::from(result.counted_filtered);
        }
    }

    Ok((reports, filtered))
}

/// Applies time-range filters to fill reports.
pub(crate) fn apply_fill_time_filters(
    mut reports: Vec<FillReport>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> Vec<FillReport> {
    match (start, end) {
        (Some(s), Some(e)) => reports.retain(|r| r.ts_event >= s && r.ts_event <= e),
        (Some(s), None) => reports.retain(|r| r.ts_event >= s),
        (None, Some(e)) => reports.retain(|r| r.ts_event <= e),
        (None, None) => {}
    }

    reports
}

fn build_position_report_from_reportable_position(
    position: &DataApiPosition,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> Option<PositionStatusReport> {
    let instrument_id = instrument_id_from_market_token(&position.condition_id, &position.asset);
    let quantity = match Quantity::from_decimal_dp(position.size, USDC_DECIMALS as u8) {
        Ok(quantity) => quantity,
        Err(e) => {
            log::warn!(
                "Skipping invalid Data API position {}-{} size {}: {e}",
                position.condition_id,
                position.asset,
                position.size,
            );
            return None;
        }
    };
    Some(PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        quantity,
        ts_init,
        ts_init,
        None,
        None,
        position.avg_price,
    ))
}

/// Cached execution state that decides which resolved Data API balances to omit.
///
/// A balance no longer represents open exposure once core settles its instrument, or once the
/// Data API marks it redeemable and no open position holds it. A redeemable balance that still
/// backs an open position stays reported until settlement, so its absence cannot be taken as a
/// flat venue position.
pub(crate) struct ResolvedBalanceScope {
    settled_instrument_ids: AHashSet<InstrumentId>,
    open_instrument_ids: AHashSet<InstrumentId>,
}

impl ResolvedBalanceScope {
    pub(crate) fn from_cache(cache: &Cache, venue: Venue, account_id: AccountId) -> Self {
        let settled_instrument_ids = cache
            .instrument_close_ids()
            .into_iter()
            .filter(|instrument_id| {
                instrument_id.venue == venue
                    && cache.instrument_close(instrument_id).is_some_and(|close| {
                        close.close_type == InstrumentCloseType::ContractExpired
                    })
            })
            .copied()
            .collect();

        let open_instrument_ids = cache
            .positions_open(Some(&venue), None, None, Some(&account_id), None)
            .iter()
            .map(|position| position.instrument_id)
            .collect();

        Self {
            settled_instrument_ids,
            open_instrument_ids,
        }
    }

    fn excludes(&self, position: &DataApiPosition, instrument_id: InstrumentId) -> bool {
        let contains = |instrument_ids: &AHashSet<InstrumentId>| {
            instrument_ids
                .iter()
                .any(|id| polymarket_instrument_ids_equivalent(*id, instrument_id))
        };

        contains(&self.settled_instrument_ids)
            || (position.redeemable && !contains(&self.open_instrument_ids))
    }
}

pub(crate) fn build_reconciliation_position_reports(
    positions: &[DataApiPosition],
    account_id: AccountId,
    ts_init: UnixNanos,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    load_ids: Option<&[InstrumentId]>,
    resolved_balances: &ResolvedBalanceScope,
) -> anyhow::Result<Vec<PositionStatusReport>> {
    let collection_load_ids = instrument_filter.is_none().then_some(load_ids).flatten();
    let mut reports = Vec::with_capacity(positions.len());

    for position in positions {
        if let Some(report) = build_reconciliation_position_report(
            position,
            account_id,
            ts_init,
            instruments,
            instrument_filter,
            collection_load_ids,
            resolved_balances,
        )? {
            reports.push(report);
        }
    }

    Ok(reports)
}

fn build_reconciliation_position_report(
    position: &DataApiPosition,
    account_id: AccountId,
    ts_init: UnixNanos,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    collection_load_ids: Option<&[InstrumentId]>,
    resolved_balances: &ResolvedBalanceScope,
) -> anyhow::Result<Option<PositionStatusReport>> {
    let instrument_id = instrument_id_from_market_token(&position.condition_id, &position.asset);

    if instrument_filter
        .is_some_and(|filter_id| !polymarket_instrument_ids_equivalent(filter_id, instrument_id))
    {
        return Ok(None);
    }

    if !instrument_in_load_ids_scope(instrument_id, collection_load_ids) {
        log::debug!("Dropping out-of-scope position instrument {instrument_id}");
        return Ok(None);
    }

    if resolved_balances.excludes(position, instrument_id) {
        log::debug!("Dropping resolved position balance for {instrument_id}");
        return Ok(None);
    }

    if position_is_dust(position) {
        return Ok(None);
    }

    if !position_instrument_loaded(&position.asset, instrument_id, instruments) {
        anyhow::bail!(unmapped_in_scope_message(
            "position",
            instrument_id,
            None,
            collection_load_ids,
        ));
    }

    Ok(build_position_report_from_reportable_position(
        position, account_id, ts_init,
    ))
}

/// Full reconciliation mass status generation.
#[expect(clippy::too_many_arguments)]
pub(crate) async fn generate_mass_status(
    http_client: &PolymarketClobHttpClient,
    data_api_client: &PolymarketDataApiHttpClient,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    fill_tracker: &OrderFillTrackerMap,
    ctx: &FillContext<'_>,
    client_id: ClientId,
    venue: Venue,
    lookback_mins: Option<u64>,
    load_ids: Option<&[InstrumentId]>,
    resolved_balances: &ResolvedBalanceScope,
) -> anyhow::Result<Option<ExecutionMassStatus>> {
    let ts_init = ctx.clock.get_time_ns();
    let lookback_start = lookback_mins
        .map(DurationNanos::try_from_mins)
        .transpose()?
        .map(|lookback| ts_init.saturating_sub(lookback));

    let orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;

    let (mut order_reports, orders_filtered) =
        build_order_reports_from_orders(&orders, instruments, ctx, None, ts_init, load_ids)?;

    let trades = http_client
        .get_trades(trades_params_for_window(
            lookback_start,
            lookback_start.map(|_| ts_init),
        ))
        .await
        .context("failed to fetch trades for mass status")?;

    let (mut fill_reports, fill_discards) = build_fill_reports_from_trades(
        &trades,
        ctx,
        instruments,
        FillReportScope::new(None, None),
        ts_init,
        load_ids,
        lookback_start,
    )?;

    if fill_discards.unowned_maker_trades > 0 {
        log::error!(
            "Mass status is missing {} confirmed maker trade(s) holding no maker order owned by \
             the account; executed quantity may be understated",
            fill_discards.unowned_maker_trades,
        );
    }

    fill_tracker.snap_fill_reports(&mut fill_reports);

    let position_reports = if ctx.signer_type == PolymarketSignerType::Session {
        Vec::new()
    } else {
        let positions = data_api_client
            .get_positions(ctx.user_address)
            .await
            .context("failed to fetch positions for mass status")?;

        build_reconciliation_position_reports(
            &positions,
            ctx.account_id,
            ts_init,
            instruments,
            None,
            load_ids,
            resolved_balances,
        )?
    };

    log::debug!(
        "Generated mass status: {} orders ({} filtered), {} fills ({} instrument-filtered, \
         {} in-scope historical misses, {} unowned maker trades, {} untimestamped trades), {} \
         positions",
        order_reports.len(),
        orders_filtered,
        fill_reports.len(),
        fill_discards.unmapped_instruments,
        fill_discards.in_scope_historical,
        fill_discards.unowned_maker_trades,
        fill_discards.untimestamped_trades,
        position_reports.len(),
    );

    if lookback_start.is_none() {
        cap_order_reports_to_confirmed_fills(&mut order_reports, &fill_reports);
    }

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    if let Some(lookback_start) = lookback_start {
        let reported_orders: AHashSet<VenueOrderId> = order_reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect();
        let reports_complete = fill_discards.in_scope_historical == 0
            && fill_discards.unowned_maker_trades == 0
            && fill_discards.untimestamped_trades == 0
            && fill_reports
                .iter()
                .all(|report| reported_orders.contains(&report.venue_order_id));
        mass_status.set_report_window(Some(lookback_start), reports_complete);
    }

    mass_status.add_order_reports(order_reports);
    mass_status.add_position_reports(position_reports);
    mass_status.add_fill_reports(fill_reports);

    Ok(Some(mass_status))
}

pub(crate) fn trades_params_for_window(
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> GetTradesParams {
    GetTradesParams {
        // CLOB `after` is exclusive of the given Unix second
        after: start.map(|ts| unix_secs(ts).saturating_sub(1)),
        before: end.map(unix_secs),
        ..Default::default()
    }
}

fn unix_secs(timestamp: UnixNanos) -> u64 {
    timestamp.as_u64() / NANOSECONDS_IN_SECOND
}

fn instrument_id_from_market_token(market: &str, token_id: &str) -> InstrumentId {
    InstrumentId::from(format!("{market}-{token_id}.POLYMARKET").as_str())
}

fn instrument_in_load_ids_scope(
    instrument_id: InstrumentId,
    load_ids: Option<&[InstrumentId]>,
) -> bool {
    match load_ids {
        Some(ids) if !ids.is_empty() => ids.iter().any(|configured_id| {
            polymarket_instrument_ids_equivalent(*configured_id, instrument_id)
        }),
        _ => true,
    }
}

fn polymarket_instrument_ids_equivalent(left: InstrumentId, right: InstrumentId) -> bool {
    if left == right {
        return true;
    }

    if left.venue != right.venue {
        return false;
    }

    let Some((left_condition, left_token)) = left.symbol.as_str().rsplit_once('-') else {
        return false;
    };
    let Some((right_condition, right_token)) = right.symbol.as_str().rsplit_once('-') else {
        return false;
    };

    left_condition.eq_ignore_ascii_case(right_condition) && left_token == right_token
}

fn unmapped_in_scope_message(
    kind: &str,
    instrument_id: InstrumentId,
    detail: Option<&str>,
    load_ids: Option<&[InstrumentId]>,
) -> String {
    let hint = match load_ids {
        Some(ids)
            if ids.iter().any(|configured_id| {
                polymarket_instrument_ids_equivalent(*configured_id, instrument_id)
            }) =>
        {
            "this instrument is in instrument_config.load_ids but was not loaded"
        }
        _ => "set instrument_config.load_ids to the instruments this node should reconcile",
    };

    match detail {
        Some(detail) => {
            format!("unmapped in-scope {kind} instrument {instrument_id} ({detail}); {hint}")
        }
        None => format!("unmapped in-scope {kind} instrument {instrument_id}; {hint}"),
    }
}

fn position_instrument_loaded(
    token_id: &str,
    instrument_id: InstrumentId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
) -> bool {
    instruments
        .get_cloned(&Ustr::from(token_id))
        .is_some_and(|instrument| {
            polymarket_instrument_ids_equivalent(instrument.id(), instrument_id)
        })
}

fn position_is_dust(position: &DataApiPosition) -> bool {
    let is_dust = position.size < DUST_POSITION_THRESHOLD;

    if is_dust && position.size > Decimal::ZERO {
        log::debug!(
            "Filtering dust position: {}-{}, size={}",
            position.condition_id,
            position.asset,
            position.size
        );
    }

    is_dust
}

fn trade_in_lookback_window(
    ts_event: Option<UnixNanos>,
    lookback_start: Option<UnixNanos>,
    in_load_ids_scope: bool,
    trade_id: &str,
    discards: &mut FillBuildDiscards,
) -> bool {
    let Some(cutoff) = lookback_start else {
        return true;
    };

    match ts_event {
        Some(ts_event) => ts_event >= cutoff,
        None => {
            if in_load_ids_scope {
                discards.untimestamped_trades += 1;
            } else {
                log::debug!(
                    "Dropping out-of-scope historical trade {trade_id} with unparsable match_time"
                );
            }
            false
        }
    }
}

fn classify_unmapped_historical(
    discards: &mut FillBuildDiscards,
    load_ids: Option<&[InstrumentId]>,
    market: &str,
    token_id: &str,
) {
    let instrument_id = instrument_id_from_market_token(market, token_id);
    discards.unmapped_instruments += 1;
    if instrument_in_load_ids_scope(instrument_id, load_ids) {
        discards.in_scope_historical += 1;
        log::warn!("Unmapped in-scope historical instrument {instrument_id}");
        return;
    }

    log::debug!("Dropping out-of-scope unmapped historical instrument {instrument_id}");
}

fn cap_order_reports_to_confirmed_fills(
    order_reports: &mut [OrderStatusReport],
    fill_reports: &[FillReport],
) {
    let confirmed_by_order = confirmed_filled_quantities(fill_reports);

    for report in order_reports {
        let local_filled = Quantity::zero(report.quantity.precision);
        cap_order_report_filled_qty(
            report,
            local_filled,
            confirmed_by_order.get(&report.venue_order_id).copied(),
        );
    }
}

pub(crate) fn confirmed_filled_quantities(
    fill_reports: &[FillReport],
) -> AHashMap<VenueOrderId, Decimal> {
    let mut confirmed_by_order = AHashMap::new();
    for fill in fill_reports {
        *confirmed_by_order.entry(fill.venue_order_id).or_default() += fill.last_qty.as_decimal();
    }

    confirmed_by_order
}

pub(crate) fn cap_order_report_filled_qty(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    confirmed_filled: Option<Decimal>,
) {
    let confirmed_filled = confirmed_filled
        .and_then(|qty| Quantity::from_decimal_dp(qty, report.quantity.precision).ok())
        .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
    let capped = report.filled_qty.min(local_filled.max(confirmed_filled));
    report.filled_qty = capped;
    normalize_terminal_order_report_quantity(report);
}

pub(crate) fn normalize_terminal_order_report_quantity(report: &mut OrderStatusReport) {
    if report.order_status != OrderStatus::Filled
        || report.filled_qty.is_zero()
        || report.filled_qty >= report.quantity
    {
        return;
    }

    let leaves = report.quantity.as_decimal() - report.filled_qty.as_decimal();
    if leaves < DUST_SNAP_THRESHOLD_DEC {
        log::debug!(
            "Normalizing terminal order report {} quantity from {} to confirmed fills {}",
            report.venue_order_id,
            report.quantity,
            report.filled_qty,
        );
        report.quantity = report.filled_qty;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::InstrumentClose,
        enums::{LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
        events::order::spec::OrderFilledSpec,
        identifiers::{PositionId, TradeId},
        position::Position,
        types::{Money, Price},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    const TEST_CONDITION_ID: &str =
        "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917";
    const TEST_TOKEN_ID: &str =
        "71321045679252212594626385532706912750332728571942532289631379312455583992563";
    const TEST_USER_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    const TEST_API_KEY: &str = "00000000-0000-0000-0000-000000000001";

    fn test_instrument() -> InstrumentAny {
        let def = crate::http::parse::PolymarketInstrumentDef {
            symbol: Ustr::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID}").as_str()),
            token_id: Ustr::from(TEST_TOKEN_ID),
            condition_id: Ustr::from(TEST_CONDITION_ID),
            market_id: "test-market".to_string(),
            event_id: None,
            gamma_market: String::new(),
            gamma_event: None,
            question_id: None,
            outcome: crate::common::enums::PolymarketOutcome::yes(),
            question: "Test market?".to_string(),
            description: None,
            price_precision: 3,
            tick_size: Decimal::new(1, 3),
            min_size: None,
            start_date: None,
            event_start_time: None,
            end_date: None,
            active: true,
            closed: false,
            market_slug: None,
            neg_risk: None,
            resolution_source: None,
            crypto_market_config: None,
            fee_schedule: None,
            game_id: None,
        };

        crate::http::parse::create_instrument_from_def(&def, UnixNanos::from(1))
            .expect("valid test instrument")
    }

    fn test_instruments() -> AtomicMap<Ustr, InstrumentAny> {
        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from(TEST_TOKEN_ID), test_instrument());
        instruments
    }

    fn test_fill_context() -> FillContext<'static> {
        FillContext {
            signer_type: PolymarketSignerType::Owner,
            account_id: AccountId::from("POLY-001"),
            user_address: TEST_USER_ADDRESS,
            api_key: TEST_API_KEY,
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        }
    }

    fn confirmed_taker_trade() -> PolymarketTradeReport {
        serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
            .expect("valid trade fixture")
    }

    fn open_order() -> PolymarketOpenOrder {
        serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
            .expect("valid open-order fixture")
    }

    fn data_api_positions() -> Vec<DataApiPosition> {
        let page: crate::http::models::DataApiPage<DataApiPosition> = serde_json::from_str(
            include_str!("../../test_data/data_api_positions_response.json"),
        )
        .expect("valid Data API position fixture");
        page.data
    }

    #[rstest]
    fn test_position_report_preserves_decimal_ingress() {
        let position: DataApiPosition = serde_json::from_str(include_str!(
            "../../test_data/decimal_precision_position.json"
        ))
        .unwrap();
        let ts = UnixNanos::from(123_456_789u64);
        let account_id = AccountId::from("POLYMARKET-001");
        let report =
            build_position_report_from_reportable_position(&position, account_id, ts).unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(
            report.instrument_id,
            instrument_id_from_market_token("0xprecision", "precision-asset")
        );
        assert_eq!(report.position_side, PositionSide::Long);
        assert_eq!(
            report.quantity.as_decimal(),
            Decimal::from_str_exact("12345678901.123456").unwrap()
        );
        assert_eq!(report.quantity.precision, USDC_DECIMALS as u8);
        assert_eq!(
            report.avg_px_open,
            Some(Decimal::from_str_exact("0.1234567890123456789012345678").unwrap())
        );
        assert_eq!(report.ts_last, ts);
        assert_eq!(report.ts_init, ts);
    }

    #[rstest]
    #[case(0, Decimal::new(55, 2))]
    #[case(2, Decimal::new(3, 1))]
    fn test_build_position_report_carries_values(
        #[case] position_index: usize,
        #[case] expected_avg_price: Decimal,
    ) {
        let positions = data_api_positions();
        let report = build_position_report_from_reportable_position(
            &positions[position_index],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("fixture position is reportable");

        assert!(report.is_long());
        assert_eq!(report.avg_px_open, Some(expected_avg_price));
        assert_eq!(report.quantity.precision, USDC_DECIMALS as u8);
    }

    #[rstest]
    fn test_build_position_report_handles_missing_avg_price() {
        let mut position = data_api_positions().remove(0);
        position.avg_price = None;

        let report = build_position_report_from_reportable_position(
            &position,
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("fixture position is reportable");

        assert_eq!(report.avg_px_open, None);
    }

    fn data_api_position(token_id: &str, redeemable: bool) -> DataApiPosition {
        DataApiPosition {
            asset: token_id.to_string(),
            condition_id: TEST_CONDITION_ID.to_string(),
            size: dec!(10),
            avg_price: Some(dec!(0.4)),
            redeemable,
        }
    }

    fn resolved_balance_scope(
        settled: Option<InstrumentId>,
        open: Option<InstrumentId>,
    ) -> ResolvedBalanceScope {
        ResolvedBalanceScope {
            settled_instrument_ids: settled.into_iter().collect(),
            open_instrument_ids: open.into_iter().collect(),
        }
    }

    #[rstest]
    #[case::unresolved(false, false, false, true)]
    #[case::settled(false, true, false, false)]
    #[case::redeemable(true, false, false, false)]
    #[case::redeemable_open_position(true, false, true, true)]
    #[case::settled_open_position(true, true, true, false)]
    fn test_position_reports_drop_resolved_balances(
        #[case] redeemable: bool,
        #[case] settled: bool,
        #[case] open: bool,
        #[case] expected_reported: bool,
    ) {
        let instrument_id = test_instrument().id();
        let position = data_api_position(TEST_TOKEN_ID, redeemable);
        let resolved_balances = resolved_balance_scope(
            settled.then_some(instrument_id),
            open.then_some(instrument_id),
        );

        let reports = build_reconciliation_position_reports(
            &[position],
            AccountId::from("POLY-001"),
            UnixNanos::from(1),
            &test_instruments(),
            None,
            None,
            &resolved_balances,
        )
        .unwrap();

        let reported_ids: Vec<_> = reports.iter().map(|report| report.instrument_id).collect();

        let expected_ids = if expected_reported {
            vec![instrument_id]
        } else {
            vec![]
        };

        assert_eq!(reported_ids, expected_ids);
    }

    #[rstest]
    #[case::settled(false, false, true)]
    #[case::open_position(true, true, false)]
    fn test_resolved_balance_scope_matches_condition_ids_ignoring_case(
        #[case] open: bool,
        #[case] redeemable: bool,
        #[case] expected_excluded: bool,
    ) {
        let instrument_id = test_instrument().id();
        let uppercase_id =
            instrument_id_from_market_token(&TEST_CONDITION_ID.to_uppercase(), TEST_TOKEN_ID);

        let resolved_balances = if open {
            resolved_balance_scope(None, Some(uppercase_id))
        } else {
            resolved_balance_scope(Some(uppercase_id), None)
        };

        let excluded = resolved_balances
            .excludes(&data_api_position(TEST_TOKEN_ID, redeemable), instrument_id);

        assert_eq!(excluded, expected_excluded);
    }

    #[rstest]
    #[case::settled(false, true, None)]
    #[case::redeemable(true, false, None)]
    #[case::unresolved(false, false, Some("unmapped in-scope position instrument"))]
    fn test_position_reports_drop_unloaded_resolved_balances(
        #[case] redeemable: bool,
        #[case] settled: bool,
        #[case] expected_error: Option<&str>,
    ) {
        let unloaded_token_id = "1234567890";
        let instrument_id = instrument_id_from_market_token(TEST_CONDITION_ID, unloaded_token_id);
        let position = data_api_position(unloaded_token_id, redeemable);
        let resolved_balances = resolved_balance_scope(settled.then_some(instrument_id), None);

        let result = build_reconciliation_position_reports(
            &[position],
            AccountId::from("POLY-001"),
            UnixNanos::from(1),
            &test_instruments(),
            None,
            None,
            &resolved_balances,
        );

        match expected_error {
            Some(expected) => {
                let error = result.unwrap_err().to_string();
                assert!(error.contains(expected), "unexpected error: {error}");
            }
            None => assert!(result.unwrap().is_empty()),
        }
    }

    #[rstest]
    fn test_resolved_balance_scope_from_cache_collects_settlements_and_open_positions() {
        let instrument = test_instrument();
        let account_id = AccountId::from("POLY-001");
        let venue = instrument.id().venue;
        let settled_id = InstrumentId::from("SETTLED-TOKEN.POLYMARKET");

        let close = |instrument_id, close_type| {
            InstrumentClose::new(
                instrument_id,
                Price::from("1.000"),
                close_type,
                UnixNanos::from(1),
                UnixNanos::from(1),
            )
        };

        let position = |account_id: AccountId, position_id: &str| {
            let fill = OrderFilledSpec::builder()
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::from(position_id))
                .account_id(account_id)
                .trade_id(TradeId::from(position_id))
                .last_qty(Quantity::from("10.000000"))
                .last_px(Price::from("0.400"))
                .currency(Currency::pUSD())
                .position_id(PositionId::from(position_id))
                .build();
            Position::new(&instrument, fill)
        };

        let mut cache = Cache::default();
        cache.add_instrument(instrument.clone()).unwrap();

        for instrument_close in [
            close(settled_id, InstrumentCloseType::ContractExpired),
            close(
                InstrumentId::from("SESSION-TOKEN.POLYMARKET"),
                InstrumentCloseType::EndOfSession,
            ),
            close(
                InstrumentId::from("OTHER-TOKEN.OTHER"),
                InstrumentCloseType::ContractExpired,
            ),
        ] {
            cache.add_instrument_close(instrument_close).unwrap();
        }

        for position in [
            position(account_id, "P-OWNED"),
            position(AccountId::from("POLY-002"), "P-FOREIGN"),
        ] {
            cache.add_position(&position, OmsType::Netting).unwrap();
        }

        let scope = ResolvedBalanceScope::from_cache(&cache, venue, account_id);

        assert_eq!(
            scope.settled_instrument_ids,
            AHashSet::from_iter([settled_id])
        );
        assert_eq!(
            scope.open_instrument_ids,
            AHashSet::from_iter([instrument.id()])
        );
    }

    #[rstest]
    fn foreign_confirmed_taker_trade_is_ignored() {
        let mut trade = confirmed_taker_trade();
        trade.maker_address = "0x1111111111111111111111111111111111111111".to_string();
        trade.owner = "foreign-api-key".to_string();

        let (reports, _) = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            FillReportScope::new(None, None),
            UnixNanos::from(1),
            None,
            None,
        )
        .expect("foreign taker trade is outside local report scope");

        assert!(reports.is_empty());
    }

    #[rstest]
    #[case(PolymarketSignerType::Owner, 1)]
    #[case(PolymarketSignerType::Session, 0)]
    fn shared_wallet_foreign_session_trade_is_not_owned(
        #[case] signer_type: PolymarketSignerType,
        #[case] expected_reports: usize,
    ) {
        let mut trade = confirmed_taker_trade();
        trade.maker_address = TEST_USER_ADDRESS.to_string();
        trade.owner = "another-session-api-key".to_string();
        let mut ctx = test_fill_context();
        ctx.signer_type = signer_type;
        let (reports, _) = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &test_instruments(),
            FillReportScope::new(None, None),
            UnixNanos::from(1),
            None,
            None,
        )
        .unwrap();
        assert_eq!(reports.len(), expected_reports);
    }

    #[rstest]
    fn confirmed_taker_trade_with_wrong_condition_fails_binding() {
        let mut trade = confirmed_taker_trade();
        trade.market =
            Ustr::from("0x1111111111111111111111111111111111111111111111111111111111111111");

        let error = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            FillReportScope::new(None, None),
            UnixNanos::from(1),
            None,
            None,
        )
        .expect_err("owned trade with contradictory condition must fail");

        assert!(error.to_string().contains("condition"));
    }

    #[rstest]
    fn confirmed_taker_trade_with_wrong_outcome_fails_binding() {
        let mut trade = confirmed_taker_trade();
        trade.outcome = crate::common::enums::PolymarketOutcome::no();

        let error = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            FillReportScope::new(None, None),
            UnixNanos::from(1),
            None,
            None,
        )
        .expect_err("owned trade with contradictory outcome must fail");

        assert!(error.to_string().contains("outcome"));
    }

    #[rstest]
    fn owned_maker_leg_with_wrong_outcome_fails_binding() {
        let mut trade = confirmed_taker_trade();
        trade.trader_side = PolymarketLiquiditySide::Maker;
        trade.maker_orders[0].owner = TEST_API_KEY.to_string();
        trade.maker_orders[0].outcome = crate::common::enums::PolymarketOutcome::no();

        let error = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            FillReportScope::new(None, None),
            UnixNanos::from(1),
            None,
            None,
        )
        .expect_err("owned maker leg with contradictory outcome must fail");

        assert!(error.to_string().contains("outcome"));
    }

    #[rstest]
    fn owned_open_order_with_wrong_condition_fails_binding() {
        let mut order = open_order();
        order.market =
            Ustr::from("0x1111111111111111111111111111111111111111111111111111111111111111");

        let error = build_order_reports_from_orders(
            &[order],
            &test_instruments(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            None,
        )
        .expect_err("owned open order with contradictory condition must fail");

        assert!(error.to_string().contains("condition"));
    }

    #[rstest]
    fn owned_open_order_with_wrong_outcome_fails_binding() {
        let mut order = open_order();
        order.outcome = crate::common::enums::PolymarketOutcome::no();

        let error = build_order_reports_from_orders(
            &[order],
            &test_instruments(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            None,
        )
        .expect_err("owned open order with contradictory outcome must fail");

        assert!(error.to_string().contains("outcome"));
    }

    #[rstest]
    fn caps_order_report_to_confirmed_companion_fills() {
        let account_id = AccountId::from("POLY-001");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let venue_order_id = VenueOrderId::from("V-1");
        let mut reports = vec![OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::PartiallyFilled,
            Quantity::from("10.0000"),
            Quantity::from("10.0000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];
        let fills = vec![FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            TradeId::from("T-1"),
            OrderSide::Buy,
            Quantity::from("4.0000"),
            Price::from("0.5000"),
            Money::zero(Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];

        cap_order_reports_to_confirmed_fills(&mut reports, &fills);

        assert_eq!(reports[0].filled_qty, Quantity::from("4.0000"));
    }

    #[rstest]
    #[case::below_threshold("99.995", "99.995")]
    #[case::at_threshold("99.990", "100.000")]
    fn normalizes_confirmed_dust_residual_to_order_quantity(
        #[case] confirmed: &str,
        #[case] expected_quantity: &str,
    ) {
        let account_id = AccountId::from("POLY-001");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let venue_order_id = VenueOrderId::from("V-DUST");
        let mut reports = vec![OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("100.000"),
            Quantity::from("100.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];
        let fills = vec![FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            TradeId::from("T-DUST"),
            OrderSide::Buy,
            Quantity::from(confirmed),
            Price::from("0.5000"),
            Money::zero(Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];

        cap_order_reports_to_confirmed_fills(&mut reports, &fills);

        assert_eq!(reports[0].quantity, Quantity::from(expected_quantity));
        assert_eq!(reports[0].filled_qty, Quantity::from(confirmed));
    }

    #[rstest]
    fn trades_params_for_window_uses_exclusive_after_unix_seconds() {
        let start = UnixNanos::from(100 * NANOSECONDS_IN_SECOND);
        let end = UnixNanos::from(250 * NANOSECONDS_IN_SECOND);

        let params = trades_params_for_window(Some(start), Some(end));

        assert_eq!(params.after, Some(99));
        assert_eq!(params.before, Some(250));
    }

    fn unmapped_open_order() -> crate::http::models::PolymarketOpenOrder {
        crate::http::models::PolymarketOpenOrder {
            associate_trades: None,
            id: "0xid".to_string(),
            status: crate::common::enums::PolymarketOrderStatus::Live,
            market: Ustr::from("0xmarket"),
            original_size: rust_decimal_macros::dec!(10),
            outcome: crate::common::enums::PolymarketOutcome::yes(),
            maker_address: TEST_USER_ADDRESS.to_string(),
            owner: TEST_API_KEY.to_string(),
            price: rust_decimal_macros::dec!(0.5),
            side: crate::common::enums::PolymarketOrderSide::Buy,
            size_matched: rust_decimal_macros::dec!(0),
            asset_id: Ustr::from("token"),
            expiration: None,
            order_type: crate::common::enums::PolymarketOrderType::GTC,
            created_at: 1_703_875_200,
        }
    }

    #[rstest]
    fn in_scope_unmapped_open_order_errors() {
        let error = build_order_reports_from_orders(
            &[unmapped_open_order()],
            &AtomicMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            None,
        )
        .expect_err("in-scope open-order miss must fail");

        let message = error.to_string();

        assert!(message.contains("unmapped in-scope open order"));
        assert!(message.contains("set instrument_config.load_ids"));
    }

    #[rstest]
    fn named_load_ids_unmapped_open_order_names_failed_load() {
        let instrument_id = InstrumentId::from("0xmarket-token.POLYMARKET");
        let error = build_order_reports_from_orders(
            &[unmapped_open_order()],
            &AtomicMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            Some(std::slice::from_ref(&instrument_id)),
        )
        .expect_err("named in-scope open-order miss must fail");
        let message = error.to_string();

        assert!(message.contains("unmapped in-scope open order"));
        assert!(message.contains("in instrument_config.load_ids but was not loaded"));
    }

    #[rstest]
    fn out_of_scope_unmapped_open_order_is_dropped() {
        let scoped = InstrumentId::from("OTHER.POLYMARKET");

        let (reports, filtered) = build_order_reports_from_orders(
            &[unmapped_open_order()],
            &AtomicMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            Some(std::slice::from_ref(&scoped)),
        )
        .expect("out-of-scope open-order miss is dropped");

        assert!(reports.is_empty());
        assert_eq!(filtered, 1);
    }
}

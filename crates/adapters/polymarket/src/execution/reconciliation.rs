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
use nautilus_core::{
    UnixNanos, collections::AtomicMap, correctness::check_valid_string_ascii,
    datetime::NANOSECONDS_IN_SECOND, time::AtomicTime,
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, PositionSideSpecified, TimeInForce},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    order_fill_tracker::OrderFillTrackerMap,
    parse::{
        OrderReportParseContext, TakerFillParseContext, build_maker_fill_report,
        determine_order_side, instrument_fee_exponent, instrument_taker_fee,
        parse_expiration_nanos, parse_timestamp, parse_validated_fill_report,
        parse_validated_order_status_report,
    },
};
use crate::{
    common::{
        consts::{DUST_POSITION_THRESHOLD, DUST_SNAP_THRESHOLD_DEC, USDC_DECIMALS},
        enums::{PolymarketLiquiditySide, PolymarketOutcome, PolymarketTradeStatus},
        models::{PolymarketMakerOrder, is_owned_by_account},
    },
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        models::{DataApiPosition, PolymarketOpenOrder, PolymarketTradeReport},
        query::{GetOrdersParams, GetTradesParams},
    },
};

/// Shared context for trade-to-fill-report conversion.
pub(crate) struct FillContext<'a> {
    pub account_id: AccountId,
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
        self.expected_order_side = expected_order_side;
        self
    }

    fn excludes_venue_order(self, venue_order_id: &str) -> bool {
        self.venue_order_id
            .is_some_and(|filter_id| venue_order_id != filter_id.as_str())
    }

    fn admits_instrument(
        self,
        instrument_id: InstrumentId,
        evidence: &str,
    ) -> anyhow::Result<bool> {
        let Some(requested_instrument_id) = self.instrument_id else {
            return Ok(true);
        };

        if instrument_id == requested_instrument_id {
            return Ok(true);
        }
        anyhow::ensure!(
            self.venue_order_id.is_none(),
            "{evidence} resolves to instrument {instrument_id}, not requested instrument {requested_instrument_id}",
        );
        Ok(false)
    }

    fn requires_target_resolution(self) -> bool {
        self.venue_order_id.is_some()
    }

    fn validate_order_side(self, actual: OrderSide, evidence: &str) -> anyhow::Result<()> {
        validate_expected_order_side(self.expected_order_side, actual, evidence)
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

fn validate_maker_order_side(
    trade: &PolymarketTradeReport,
    maker_order: &PolymarketMakerOrder,
) -> anyhow::Result<OrderSide> {
    let derived_side = determine_order_side(
        trade.trader_side,
        trade.side,
        trade.asset_id.as_str(),
        maker_order.asset_id.as_str(),
    );
    let provider_side = maker_order
        .side
        .with_context(|| format!("REST maker order {} is missing side", maker_order.order_id))?;
    anyhow::ensure!(
        OrderSide::from(provider_side) == derived_side,
        "provider maker order {} side {provider_side} contradicts derived side {derived_side}",
        maker_order.order_id,
    );
    Ok(derived_side)
}

fn checked_venue_order_id(value: &str, evidence: &str) -> anyhow::Result<VenueOrderId> {
    VenueOrderId::new_checked(value)
        .with_context(|| format!("{evidence} has invalid venue order ID {value:?}"))
}

fn checked_trade_id(value: &str, evidence: &str) -> anyhow::Result<TradeId> {
    TradeId::new_checked(value)
        .with_context(|| format!("{evidence} has invalid trade ID {value:?}"))
}

fn validate_maker_report_identifiers(
    trade: &PolymarketTradeReport,
    maker_order: &PolymarketMakerOrder,
) -> anyhow::Result<()> {
    checked_venue_order_id(
        &maker_order.order_id,
        &format!("maker order in trade {}", trade.id),
    )?;
    check_valid_string_ascii(&trade.id, "trade.id")
        .with_context(|| format!("maker trade {} has invalid trade ID source", trade.id))?;
    Ok(())
}

fn validate_instrument_binding(
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
        instrument_outcome.as_str() == outcome.as_str(),
        "provider outcome {outcome} does not match instrument outcome {instrument_outcome}",
    );

    Ok(())
}

fn validate_quantity_evidence(
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

fn validate_price_evidence(value: Decimal, precision: u8, field: &str) -> anyhow::Result<()> {
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
        None | Some("0") => Ok(None),
        Some(value) => parse_expiration_nanos(value)
            .map(UnixNanos::from)
            .map(Some)
            .with_context(|| format!("provider order {} has invalid expiration {value}", order.id)),
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

fn validate_trade_values(
    quantity: Decimal,
    price: Decimal,
    price_precision: u8,
    size_precision: u8,
    quantity_field: &str,
    price_field: &str,
) -> anyhow::Result<()> {
    validate_quantity_evidence(quantity, size_precision, quantity_field, false)?;
    validate_price_evidence(price, price_precision, price_field)
}

fn validate_pending_trade_values(
    quantity: Decimal,
    price: Decimal,
    quantity_field: &str,
    price_field: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        quantity > Decimal::ZERO,
        "{quantity_field} {quantity} must be positive"
    );
    anyhow::ensure!(
        price > Decimal::ZERO && price < Decimal::ONE,
        "{price_field} {price} must be greater than zero and less than one",
    );
    Ok(())
}

fn require_trade_timestamp(
    ts_event: Option<UnixNanos>,
    trade: &PolymarketTradeReport,
) -> anyhow::Result<UnixNanos> {
    ts_event.with_context(|| {
        format!(
            "selected trade {} has invalid match_time {}",
            trade.id, trade.match_time,
        )
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

fn validate_client_bound_order_row(
    provider_order: &PolymarketOpenOrder,
    cached_order: &OrderAny,
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
    anyhow::ensure!(
        cached_order.quantity().as_decimal() == provider_order.original_size,
        "provider order quantity {} does not match cached order quantity {}",
        provider_order.original_size,
        cached_order.quantity(),
    );
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
}

impl<'a> TargetOrderReportScope<'a> {
    pub(crate) const fn new(
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        cached_order: Option<&'a OrderAny>,
    ) -> Self {
        Self {
            instrument_id,
            venue_order_id,
            client_order_id,
            cached_order,
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
        OrderEvidenceScope::Collection { .. } => match instruments.get_cloned(&order.asset_id) {
            Some(instrument) => instrument,
            None => {
                let instrument_id =
                    instrument_id_from_market_token(order.market.as_str(), order.asset_id.as_str());

                if instrument_in_load_ids_scope(instrument_id, load_ids) {
                    anyhow::bail!(unmapped_in_scope_message(
                        "open order",
                        instrument_id,
                        Some(&format!("token {}", order.asset_id)),
                        load_ids,
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

    if let OrderEvidenceScope::Collection { instrument_filter } = scope
        && instrument_filter.is_some_and(|filter_id| instrument_id != filter_id)
    {
        return Ok(OrderRowResult {
            report: None,
            counted_filtered: false,
        });
    }

    validate_instrument_binding(&instrument, order.market.as_str(), order.outcome)?;
    let validated = validate_order_row_values(
        order,
        instrument.price_precision(),
        instrument.size_precision(),
    )?;
    let (client_order_id, cached_order) = match scope {
        OrderEvidenceScope::Collection { .. } => (None, None),
        OrderEvidenceScope::Target {
            client_order_id,
            cached_order,
            ..
        } => (client_order_id, cached_order),
    };

    if let Some(cached_order) = cached_order {
        validate_client_bound_order_row(order, cached_order, validated.expire_time)?;
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
    );
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
    build_order_report_from_order(
        order,
        instruments,
        ctx,
        OrderEvidenceScope::Target {
            instrument_id: scope.instrument_id,
            venue_order_id: scope.venue_order_id,
            client_order_id: scope.client_order_id,
            cached_order: scope.cached_order,
        },
        ts_init,
        None,
    )?
    .report
    .context("target order evidence was unexpectedly ignored")
}

#[derive(Clone, Copy)]
enum TargetFillParticipant<'a> {
    Taker {
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
    },
    Maker(&'a PolymarketMakerOrder),
}

struct AdmittedTargetFill<'a> {
    participant: TargetFillParticipant<'a>,
    instrument: InstrumentAny,
    ts_event: UnixNanos,
}

#[derive(Clone, Copy)]
enum TargetTradeClass {
    Unrelated,
    Pending,
    Confirmed,
    Failed,
}

struct TargetTradeAdmission<'a> {
    class: TargetTradeClass,
    confirmed_fill: Option<AdmittedTargetFill<'a>>,
}

fn classify_target_trade<'a>(
    trade: &'a PolymarketTradeReport,
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_id: Option<InstrumentId>,
    venue_order_id: VenueOrderId,
    expected_order_side: Option<OrderSide>,
) -> anyhow::Result<TargetTradeAdmission<'a>> {
    if !validate_target_trade_role(trade, venue_order_id)? {
        return Ok(TargetTradeAdmission {
            class: TargetTradeClass::Unrelated,
            confirmed_fill: None,
        });
    }

    let (participant, instrument, quantity, price, quantity_field, price_field) =
        if trade.trader_side == PolymarketLiquiditySide::Maker {
            let maker_order = trade
                .maker_orders
                .iter()
                .find(|order| order.order_id == venue_order_id.as_str())
                .context("validated target maker occurrence is missing")?;
            anyhow::ensure!(
                maker_order.is_owned_by(ctx.user_address, ctx.api_key),
                "target maker order {} is not owned by the account",
                maker_order.order_id,
            );
            validate_maker_report_identifiers(trade, maker_order)?;
            let instrument = resolve_target_instrument(
                instruments,
                maker_order.asset_id,
                instrument_id,
                &format!(
                    "target maker trade {} order {}",
                    trade.id, maker_order.order_id
                ),
            )?;
            validate_instrument_binding(&instrument, trade.market.as_str(), maker_order.outcome)?;
            let order_side = validate_maker_order_side(trade, maker_order)?;
            validate_expected_order_side(
                expected_order_side,
                order_side,
                &format!("target maker order {}", maker_order.order_id),
            )?;
            (
                TargetFillParticipant::Maker(maker_order),
                instrument,
                maker_order.matched_amount,
                maker_order.price,
                format!("target maker order {} matched amount", maker_order.order_id),
                format!("target maker order {} price", maker_order.order_id),
            )
        } else {
            anyhow::ensure!(
                is_owned_by_account(
                    &trade.maker_address,
                    &trade.owner,
                    ctx.user_address,
                    ctx.api_key,
                ),
                "target taker order {} is not owned by the account",
                trade.taker_order_id,
            );
            let venue_order_id =
                checked_venue_order_id(&trade.taker_order_id, "target taker trade")?;
            let trade_id = checked_trade_id(&trade.id, "target taker trade")?;
            let instrument = resolve_target_instrument(
                instruments,
                trade.asset_id,
                instrument_id,
                &format!("target taker trade {}", trade.id),
            )?;
            validate_instrument_binding(&instrument, trade.market.as_str(), trade.outcome)?;
            validate_expected_order_side(
                expected_order_side,
                OrderSide::from(trade.side),
                &format!("target taker order {}", trade.taker_order_id),
            )?;
            (
                TargetFillParticipant::Taker {
                    venue_order_id,
                    trade_id,
                },
                instrument,
                trade.size,
                trade.price,
                format!("target taker trade {} size", trade.id),
                format!("target taker trade {} price", trade.id),
            )
        };

    match trade.status {
        PolymarketTradeStatus::Matched
        | PolymarketTradeStatus::Mined
        | PolymarketTradeStatus::Retrying => {
            validate_pending_trade_values(
                quantity,
                price,
                &format!("pending {quantity_field}"),
                &format!("pending {price_field}"),
            )?;
            require_trade_timestamp(parse_timestamp(&trade.match_time), trade)?;
            Ok(TargetTradeAdmission {
                class: TargetTradeClass::Pending,
                confirmed_fill: None,
            })
        }
        PolymarketTradeStatus::Confirmed => {
            validate_trade_values(
                quantity,
                price,
                instrument.price_precision(),
                instrument.size_precision(),
                &quantity_field,
                &price_field,
            )?;
            let ts_event = require_trade_timestamp(parse_timestamp(&trade.match_time), trade)?;
            Ok(TargetTradeAdmission {
                class: TargetTradeClass::Confirmed,
                confirmed_fill: Some(AdmittedTargetFill {
                    participant,
                    instrument,
                    ts_event,
                }),
            })
        }
        PolymarketTradeStatus::Failed => Ok(TargetTradeAdmission {
            class: TargetTradeClass::Failed,
            confirmed_fill: None,
        }),
    }
}

fn build_admitted_target_fill(
    trade: &PolymarketTradeReport,
    admitted: &AdmittedTargetFill<'_>,
    ctx: &FillContext<'_>,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = admitted.instrument.id();
    let price_prec = admitted.instrument.price_precision();
    let size_prec = admitted.instrument.size_precision();

    match admitted.participant {
        TargetFillParticipant::Maker(maker_order) => build_maker_fill_report(
            maker_order,
            &trade.id,
            trade.trader_side,
            trade.side,
            trade.asset_id.as_str(),
            ctx.account_id,
            instrument_id,
            price_prec,
            size_prec,
            ctx.pusd,
            LiquiditySide::Maker,
            admitted.ts_event,
            ts_init,
        )
        .with_context(|| {
            format!(
                "failed to build target maker fill report for trade {} and order {}",
                trade.id, maker_order.order_id,
            )
        }),
        TargetFillParticipant::Taker {
            venue_order_id,
            trade_id,
        } => {
            let taker_fee_rate = instrument_taker_fee(&admitted.instrument);
            let fee_exponent = instrument_fee_exponent(&admitted.instrument);
            parse_validated_fill_report(
                trade,
                TakerFillParseContext {
                    instrument_id,
                    account_id: ctx.account_id,
                    client_order_id: None,
                    venue_order_id,
                    trade_id,
                    price_precision: price_prec,
                    size_precision: size_prec,
                    currency: ctx.pusd,
                    taker_fee_rate,
                    fee_exponent,
                    ts_event: admitted.ts_event,
                    ts_init,
                },
            )
            .with_context(|| format!("failed to build target taker fill for trade {}", trade.id))
        }
    }
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

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    scope: FillReportScope,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
    lookback_start: Option<UnixNanos>,
) -> anyhow::Result<(Vec<FillReport>, FillBuildDiscards)> {
    let mut reports = Vec::new();
    let mut discards = FillBuildDiscards::default();

    for trade in trades {
        if let Some(target_order_id) = scope.venue_order_id {
            let admission = classify_target_trade(
                trade,
                ctx,
                instruments,
                scope.instrument_id,
                target_order_id,
                scope.expected_order_side,
            )?;

            match admission.class {
                TargetTradeClass::Unrelated | TargetTradeClass::Failed => continue,
                TargetTradeClass::Pending => {
                    discards.has_pending_target = true;
                    continue;
                }
                TargetTradeClass::Confirmed => {
                    let admitted = admission
                        .confirmed_fill
                        .as_ref()
                        .context("confirmed target admission is missing fill evidence")?;
                    reports.push(build_admitted_target_fill(trade, admitted, ctx, ts_init)?);
                    continue;
                }
            }
        }

        if trade.status != PolymarketTradeStatus::Confirmed {
            continue;
        }

        let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

        if is_maker {
            if !trade
                .maker_orders
                .iter()
                .any(|mo| mo.is_owned_by(ctx.user_address, ctx.api_key))
            {
                if let Some(target_order_id) = scope.venue_order_id {
                    anyhow::bail!(
                        "target maker order {target_order_id} is not owned by the account"
                    );
                }
                let ts_event = parse_timestamp(&trade.match_time);
                let instrument_id =
                    instrument_id_from_market_token(trade.market.as_str(), trade.asset_id.as_str());
                let in_load_ids_scope = instrument_in_load_ids_scope(instrument_id, load_ids);

                if !trade_in_lookback_window(
                    ts_event,
                    lookback_start,
                    in_load_ids_scope,
                    &trade.id,
                    &mut discards,
                ) {
                    continue;
                }
                discards.unowned_maker_trades += 1;
                log::debug!(
                    "Confirmed maker trade {} holds no maker order owned by the account",
                    trade.id,
                );
                continue;
            }

            let mut selected_maker_orders: Vec<(&PolymarketMakerOrder, InstrumentAny)> = Vec::new();

            for mo in &trade.maker_orders {
                if scope.excludes_venue_order(&mo.order_id) {
                    continue;
                }

                if !mo.is_owned_by(ctx.user_address, ctx.api_key) {
                    anyhow::ensure!(
                        !scope.requires_target_resolution(),
                        "target maker order {} is not owned by the account",
                        mo.order_id,
                    );
                    continue;
                }
                let token_id = mo.asset_id;
                let instrument = match instruments.get_cloned(&token_id) {
                    Some(instrument) => instrument,
                    None => {
                        anyhow::ensure!(
                            !scope.requires_target_resolution(),
                            "target maker order {} token {token_id} has no loaded Polymarket instrument",
                            mo.order_id,
                        );
                        classify_unmapped_historical(
                            &mut discards,
                            load_ids,
                            &trade.market,
                            token_id.as_str(),
                        );
                        continue;
                    }
                };
                let instrument_id = instrument.id();

                if !scope.admits_instrument(
                    instrument_id,
                    &format!("target maker order {}", mo.order_id),
                )? {
                    continue;
                }

                anyhow::ensure!(
                    !selected_maker_orders
                        .iter()
                        .any(|(selected, _)| selected.order_id == mo.order_id),
                    "maker order {} appears more than once in trade {}",
                    mo.order_id,
                    trade.id,
                );

                validate_maker_report_identifiers(trade, mo)?;

                validate_instrument_binding(&instrument, trade.market.as_str(), mo.outcome)?;
                let order_side = validate_maker_order_side(trade, mo)?;
                scope.validate_order_side(
                    order_side,
                    &format!("target maker order {}", mo.order_id),
                )?;
                validate_trade_values(
                    mo.matched_amount,
                    mo.price,
                    instrument.price_precision(),
                    instrument.size_precision(),
                    &format!("maker order {} matched amount", mo.order_id),
                    &format!("maker order {} price", mo.order_id),
                )?;
                selected_maker_orders.push((mo, instrument));
            }

            if selected_maker_orders.is_empty() {
                continue;
            }

            let ts_event = parse_timestamp(&trade.match_time);
            let in_load_ids_scope = selected_maker_orders
                .iter()
                .any(|(_, instrument)| instrument_in_load_ids_scope(instrument.id(), load_ids));

            if !trade_in_lookback_window(
                ts_event,
                lookback_start,
                in_load_ids_scope,
                &trade.id,
                &mut discards,
            ) {
                continue;
            }
            let ts_event = require_trade_timestamp(ts_event, trade)?;

            for (mo, instrument) in selected_maker_orders {
                let instrument_id = instrument.id();
                let price_prec = instrument.price_precision();
                let size_prec = instrument.size_precision();

                let report = build_maker_fill_report(
                    mo,
                    &trade.id,
                    trade.trader_side,
                    trade.side,
                    trade.asset_id.as_str(),
                    ctx.account_id,
                    instrument_id,
                    price_prec,
                    size_prec,
                    ctx.pusd,
                    LiquiditySide::Maker,
                    ts_event,
                    ts_init,
                )
                .with_context(|| {
                    format!(
                        "failed to build maker fill report for trade {} and order {}",
                        trade.id, mo.order_id,
                    )
                })?;
                reports.push(report);
            }
        } else {
            if scope.excludes_venue_order(&trade.taker_order_id) {
                continue;
            }

            if !is_owned_by_account(
                &trade.maker_address,
                &trade.owner,
                ctx.user_address,
                ctx.api_key,
            ) {
                anyhow::ensure!(
                    !scope.requires_target_resolution(),
                    "target taker order {} is not owned by the account",
                    trade.taker_order_id,
                );
                log::debug!(
                    "Dropping confirmed taker trade {} not owned by the account",
                    trade.id
                );
                continue;
            }

            let token_id = trade.asset_id;
            let instrument = match instruments.get_cloned(&token_id) {
                Some(instrument) => instrument,
                None => {
                    anyhow::ensure!(
                        !scope.requires_target_resolution(),
                        "target taker order {} token {token_id} has no loaded Polymarket instrument",
                        trade.taker_order_id,
                    );
                    classify_unmapped_historical(
                        &mut discards,
                        load_ids,
                        &trade.market,
                        token_id.as_str(),
                    );
                    continue;
                }
            };
            let instrument_id = instrument.id();

            if !scope.admits_instrument(
                instrument_id,
                &format!("target taker order {}", trade.taker_order_id),
            )? {
                continue;
            }

            let venue_order_id = checked_venue_order_id(&trade.taker_order_id, "taker trade")?;
            let trade_id = checked_trade_id(&trade.id, "taker trade")?;

            validate_instrument_binding(&instrument, trade.market.as_str(), trade.outcome)?;
            validate_trade_values(
                trade.size,
                trade.price,
                instrument.price_precision(),
                instrument.size_precision(),
                &format!("taker trade {} size", trade.id),
                &format!("taker trade {} price", trade.id),
            )?;
            let ts_event = parse_timestamp(&trade.match_time);
            let in_load_ids_scope = instrument_in_load_ids_scope(instrument_id, load_ids);

            if !trade_in_lookback_window(
                ts_event,
                lookback_start,
                in_load_ids_scope,
                &trade.id,
                &mut discards,
            ) {
                continue;
            }
            let ts_event = require_trade_timestamp(ts_event, trade)?;
            let price_prec = instrument.price_precision();
            let size_prec = instrument.size_precision();
            let taker_fee_rate = instrument_taker_fee(&instrument);
            let fee_exponent = instrument_fee_exponent(&instrument);

            let report = parse_validated_fill_report(
                trade,
                TakerFillParseContext {
                    instrument_id,
                    account_id: ctx.account_id,
                    client_order_id: None,
                    venue_order_id,
                    trade_id,
                    price_precision: price_prec,
                    size_precision: size_prec,
                    currency: ctx.pusd,
                    taker_fee_rate,
                    fee_exponent,
                    ts_event,
                    ts_init,
                },
            )
            .with_context(|| format!("failed to build taker fill report for trade {}", trade.id))?;
            scope.validate_order_side(
                report.order_side,
                &format!("target taker order {}", trade.taker_order_id),
            )?;
            reports.push(report);
        }
    }

    Ok((reports, discards))
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

/// Builds position status reports from Data API positions, filtering dust.
pub(crate) fn build_position_reports(
    positions: &[DataApiPosition],
    account_id: AccountId,
    ts: UnixNanos,
) -> Vec<PositionStatusReport> {
    positions
        .iter()
        .filter(|p| {
            if p.size > Decimal::ZERO && p.size < DUST_POSITION_THRESHOLD {
                log::debug!(
                    "Filtering dust position: {}-{}, size={}",
                    p.condition_id,
                    p.asset,
                    p.size
                );
            }
            p.size >= DUST_POSITION_THRESHOLD
        })
        .filter_map(|p| {
            let instrument_id = instrument_id_from_market_token(&p.condition_id, &p.asset);
            let quantity = match Quantity::from_decimal_dp(p.size, USDC_DECIMALS as u8) {
                Ok(quantity) => quantity,
                Err(e) => {
                    log::warn!(
                        "Skipping invalid Data API position {}-{} size {}: {e}",
                        p.condition_id,
                        p.asset,
                        p.size,
                    );
                    return None;
                }
            };
            Some(PositionStatusReport::new(
                account_id,
                instrument_id,
                PositionSideSpecified::Long,
                quantity,
                ts,
                ts,
                None,
                None,
                p.avg_price,
            ))
        })
        .collect()
}

pub(crate) fn retain_mapped_position_reports(
    reports: Vec<PositionStatusReport>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<Vec<PositionStatusReport>> {
    let mut kept = Vec::with_capacity(reports.len());

    for report in reports {
        if position_instrument_loaded(report.instrument_id, instruments) {
            kept.push(report);
            continue;
        }

        if instrument_in_load_ids_scope(report.instrument_id, load_ids) {
            anyhow::bail!(unmapped_in_scope_message(
                "position",
                report.instrument_id,
                None,
                load_ids,
            ));
        }
        log::debug!(
            "Dropping out-of-scope unmapped position instrument {}",
            report.instrument_id
        );
    }

    Ok(kept)
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
) -> anyhow::Result<Option<ExecutionMassStatus>> {
    let ts_init = ctx.clock.get_time_ns();
    let lookback_start = lookback_mins.map(|mins| {
        UnixNanos::from(
            ts_init.as_u64().saturating_sub(
                mins.saturating_mul(60)
                    .saturating_mul(NANOSECONDS_IN_SECOND),
            ),
        )
    });

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

    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;

    let position_reports = retain_mapped_position_reports(
        build_position_reports(&positions, ctx.account_id, ts_init),
        instruments,
        load_ids,
    )?;

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

fn unix_secs(ts: UnixNanos) -> u64 {
    ts.as_u64() / NANOSECONDS_IN_SECOND
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
    instrument_id: InstrumentId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
) -> bool {
    let symbol = instrument_id.symbol.as_str();
    symbol
        .rsplit_once('-')
        .is_some_and(|(_, token_id)| instruments.contains_key(&Ustr::from(token_id)))
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
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::TradeId,
        types::{Money, Price},
    };
    use rstest::rstest;

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
            question_id: None,
            outcome: crate::common::enums::PolymarketOutcome::yes(),
            question: "Test market?".to_string(),
            description: None,
            price_precision: 3,
            tick_size: Decimal::new(1, 3),
            min_size: None,
            maker_fee: None,
            taker_fee: None,
            start_date: None,
            end_date: None,
            active: true,
            closed: false,
            market_slug: None,
            neg_risk: None,
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
            OrderSide::Buy,
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
            OrderSide::Buy,
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

    #[rstest]
    fn in_scope_unmapped_position_errors() {
        let reports = vec![PositionStatusReport::new(
            AccountId::from("POLY-001"),
            InstrumentId::from("0xmarket-token.POLYMARKET"),
            PositionSideSpecified::Long,
            Quantity::from("10.000000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
            None,
            None,
        )];

        let error = retain_mapped_position_reports(reports, &AtomicMap::new(), None)
            .expect_err("in-scope position miss must fail");

        let message = error.to_string();

        assert!(message.contains("unmapped in-scope position"));
        assert!(message.contains("set instrument_config.load_ids"));
    }
}

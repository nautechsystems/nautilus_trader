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
use nautilus_common::messages::execution::{
    GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
    GeneratePositionStatusReports, QueryAccount, QueryOrder,
};
use nautilus_core::{UnixNanos, collections::AtomicMap, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    PolymarketExecutionClient,
    parse::{
        parse_balance_allowance, parse_order_status_report, recovered_terminal_order_status,
        sum_filled_quantity, weighted_average_price,
    },
    reconciliation::{
        FillContext, apply_fill_filters, build_fill_reports_from_trades,
        build_position_reports_scoped, cap_order_report_filled_qty, confirmed_filled_quantities,
        confirmed_trade_in_static_scope,
    },
    report_validation::{ensure_instrument_binding, non_negative_quantity, parse_match_time},
};
use crate::{
    common::enums::SignatureType,
    http::{
        clob::PolymarketClobHttpClient,
        models::{PolymarketOpenOrder, PolymarketTradeReport},
        query::{GetBalanceAllowanceParams, GetTradesParams},
    },
};

impl PolymarketExecutionClient {
    pub(super) fn fill_context(&self) -> FillContext<'_> {
        let user_address = self
            .secrets
            .funder
            .as_deref()
            .unwrap_or(&self.secrets.address);
        FillContext {
            account_id: self.core.account_id,
            user_address,
            api_key: self.secrets.credential.api_key().as_str(),
            clock: self.clock,
        }
    }

    pub(super) async fn recover_terminal_status_from_trades(
        &self,
        venue_order_id: VenueOrderId,
        instrument: &InstrumentAny,
        client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let instrument_id = instrument.id();
        let size_prec = instrument.size_precision();
        let ts_init = self.clock.get_time_ns();
        let ctx = self.fill_context();

        let trades = self
            .http_client
            .get_trades(GetTradesParams::default())
            .await
            .context("failed to fetch trades for order recovery")?;

        let resolved_client_order_id =
            client_order_id.or_else(|| self.core.cache().client_order_id(&venue_order_id).copied());
        let cached = resolved_client_order_id.and_then(|cid| self.core.cache().order_owned(&cid));
        if let Some(cached) = cached.as_ref() {
            anyhow::ensure!(
                cached.instrument_id() == instrument_id,
                "tracked order instrument {} does not match requested recovery instrument {instrument_id}",
                cached.instrument_id(),
            );
        }
        let cached_quantity = cached.as_ref().map(Order::quantity);
        let cached_order_type = cached.as_ref().map_or(OrderType::Limit, Order::order_type);
        let cached_tif = cached
            .as_ref()
            .map_or(TimeInForce::Gtc, Order::time_in_force);
        let cached_price = cached.as_ref().and_then(Order::price);
        let cached_side = cached.as_ref().map(Order::order_side);

        let has_pending_trade =
            trades
                .iter()
                .try_fold(false, |found, trade| -> anyhow::Result<bool> {
                    if !trade.status.is_pending_settlement() {
                        return Ok(found);
                    }
                    let matches = pending_trade_matches_known_order(
                        trade,
                        venue_order_id,
                        instrument,
                        cached_side,
                    )?;
                    Ok(found || matches)
                })?;

        if has_pending_trade {
            let Some(cached) = cached.as_ref() else {
                log::debug!(
                    "Order {venue_order_id} has unsettled trades but no cached order; deferring recovery"
                );
                return Ok(None);
            };
            let order_status = if cached.filled_qty().is_zero() {
                OrderStatus::Accepted
            } else {
                OrderStatus::PartiallyFilled
            };
            let mut report = OrderStatusReport::new(
                self.core.account_id,
                instrument_id,
                resolved_client_order_id,
                venue_order_id,
                cached.order_side(),
                cached.order_type(),
                cached.time_in_force(),
                order_status,
                cached.quantity(),
                cached.filled_qty(),
                ts_init,
                ts_init,
                ts_init,
                None,
            );
            report.price = cached_price;

            log::debug!(
                "Order {venue_order_id} has unsettled trades; reporting non-terminal {order_status}"
            );
            return Ok(Some(report));
        }

        let (mut order_fills, _) = build_fill_reports_from_trades(
            &trades,
            &ctx,
            &self.shared_token_instruments,
            Some(instrument_id),
            ts_init,
            self.config.reconciliation_load_ids(),
        )?;
        order_fills.retain(|f| f.venue_order_id == venue_order_id);
        self.fill_tracker.snap_fill_reports(&mut order_fills);

        if order_fills.is_empty() {
            let Some(cached) = cached.as_ref() else {
                log::debug!(
                    "Order {venue_order_id} not active at venue, no trades found, and no cached order; nothing to recover"
                );
                return Ok(None);
            };
            log::debug!(
                "Order {venue_order_id} not active at venue and no trades found; recovering as Canceled"
            );
            let mut report = OrderStatusReport::new(
                self.core.account_id,
                instrument_id,
                resolved_client_order_id,
                venue_order_id,
                cached.order_side(),
                cached.order_type(),
                cached.time_in_force(),
                OrderStatus::Canceled,
                cached.quantity(),
                cached.filled_qty(),
                ts_init,
                ts_init,
                ts_init,
                None,
            );
            report.price = cached_price;
            report.cancel_reason = Some("ORDER_NOT_FOUND_AT_VENUE".to_string());
            return Ok(Some(report));
        }

        let Some(quantity) = cached_quantity else {
            log::debug!(
                "Order {venue_order_id} has trades but no cached order; deferring to engine"
            );
            return Ok(None);
        };

        let total_filled_dec = sum_filled_quantity(&order_fills)?;
        let avg_px = weighted_average_price(&order_fills, total_filled_dec)?;
        let raw_filled_qty =
            non_negative_quantity(total_filled_dec, size_prec, "recovered filled quantity")?;
        let order_side = cached_side.unwrap_or(order_fills[0].order_side);
        let ts_event = order_fills
            .iter()
            .map(|f| f.ts_event)
            .max()
            .unwrap_or(ts_init);

        let order_status = recovered_terminal_order_status(cached_tif, quantity, raw_filled_qty);
        let filled_qty = raw_filled_qty;

        log::debug!(
            "Recovered {} status for {venue_order_id} from {} trade(s) (filled_qty={filled_qty}, quantity={quantity})",
            if order_status == OrderStatus::Filled {
                "Filled"
            } else {
                "Canceled (partially filled)"
            },
            order_fills.len(),
        );

        let mut report = OrderStatusReport::new(
            self.core.account_id,
            instrument_id,
            resolved_client_order_id,
            venue_order_id,
            order_side,
            cached_order_type,
            cached_tif,
            order_status,
            quantity,
            filled_qty,
            ts_event,
            ts_event,
            ts_init,
            None,
        );
        report.price = cached_price;
        report.avg_px = avg_px;
        let cached_filled = cached
            .as_ref()
            .map_or_else(|| Quantity::zero(size_prec), |order| order.filled_qty());
        let tracked_filled = self
            .fill_tracker
            .get_cumulative_filled(&venue_order_id)
            .unwrap_or_else(|| Quantity::zero(size_prec));
        cap_order_report_filled_qty(
            &mut report,
            cached_filled,
            tracked_filled,
            Some(total_filled_dec),
        )?;

        Ok(Some(report))
    }

    pub(super) fn query_account_command(&self, _cmd: QueryAccount) {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;

        self.spawn_task("query_account", async move {
            fetch_and_emit_account_state(&http_client, &emitter, clock, signature_type).await
        });
    }

    pub(super) fn query_order_command(&self, cmd: &QueryOrder) {
        log::debug!("Querying order: client_order_id={}", cmd.client_order_id);

        let Some(venue_order_id) =
            self.resolve_venue_order_id(cmd.venue_order_id, Some(cmd.client_order_id))
        else {
            log::warn!(
                "query_order requires a venue_order_id for Polymarket: {}",
                cmd.client_order_id
            );
            return;
        };
        let venue_order_id = venue_order_id.to_string();

        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let account_id = self.core.account_id;
        let cache = self.core.cache();

        let instrument = match cache.instrument(&instrument_id).cloned() {
            Some(instrument) => instrument,
            None => {
                log::warn!("Cannot query order for unloaded instrument {instrument_id}");
                return;
            }
        };
        let size_prec = instrument.size_precision();
        let expected_order = cache.order(&client_order_id).map(|order| order.cloned());

        let http_client = self.http_client.clone();
        let fill_tracker = self.fill_tracker.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let api_key = self.secrets.credential.api_key().to_string();
        let load_ids = self.config.reconciliation_load_ids().map(Vec::from);
        let cached_filled = cache
            .order(&client_order_id)
            .map_or_else(|| Quantity::zero(size_prec), |order| order.filled_qty());

        self.spawn_task("query_order", async move {
            match http_client.get_order_optional(&venue_order_id).await {
                Ok(Some(order)) => {
                    let mut report = match parse_order_status_report(
                        &order,
                        &instrument,
                        account_id,
                        Some(client_order_id),
                        clock.get_time_ns(),
                    ) {
                        Ok(report) => report,
                        Err(e) => {
                            log::warn!("Failed to validate queried order {venue_order_id}: {e}");
                            return Ok(());
                        }
                    };
                    let requested_venue_order_id = VenueOrderId::from(venue_order_id.as_str());

                    if let Err(e) = validate_order_response_scope(
                        &order,
                        &report,
                        requested_venue_order_id,
                        &instrument,
                        expected_order.as_ref(),
                    ) {
                        log::warn!("Rejected queried order {venue_order_id}: {e}");
                        return Ok(());
                    }
                    let venue_order_id = requested_venue_order_id;
                    let tracked_filled = fill_tracker
                        .get_cumulative_filled(&venue_order_id)
                        .unwrap_or_else(|| Quantity::zero(size_prec));
                    let local_filled = cached_filled.max(tracked_filled);
                    let confirmed_filled = if report.filled_qty > local_filled {
                        let ctx = FillContext {
                            account_id,
                            user_address: &user_address,
                            api_key: &api_key,
                            clock,
                        };

                        match fetch_confirmed_fill_reports(
                            &http_client,
                            &ctx,
                            &token_instruments,
                            GetTradesParams::default(),
                            Some(instrument_id),
                            clock.get_time_ns(),
                            load_ids.as_deref(),
                        )
                        .await
                        {
                            Ok(fills) => confirmed_filled_quantities(&fills)?
                                .get(&venue_order_id)
                                .copied(),
                            Err(e) => {
                                log::warn!(
                                    "Failed to fetch confirmed fills for order {venue_order_id}: {e}"
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };
                    cap_order_report_filled_qty(
                        &mut report,
                        cached_filled,
                        tracked_filled,
                        confirmed_filled,
                    )?;
                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::warn!("Order {venue_order_id} not found (empty response)");
                }
                Err(e) => {
                    log::warn!("Failed to query order {venue_order_id}: {e}");
                }
            }
            Ok(())
        });
    }

    pub(super) async fn generate_order_status_report_impl(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(venue_order_id) =
            self.resolve_venue_order_id(cmd.venue_order_id, cmd.client_order_id)
        else {
            anyhow::bail!("generate_order_status_report requires venue_order_id");
        };

        let Some(instrument_id) = cmd.instrument_id else {
            anyhow::bail!("generate_order_status_report requires instrument_id");
        };

        let order = self
            .http_client
            .get_order_optional(venue_order_id.as_str())
            .await
            .context("failed to fetch order")?;

        let instrument = self
            .core
            .cache()
            .instrument(&instrument_id)
            .cloned()
            .with_context(|| format!("instrument {instrument_id} is not loaded"))?;
        let size_prec = instrument.size_precision();

        if let Some(order) = order {
            let mut report = parse_order_status_report(
                &order,
                &instrument,
                self.core.account_id,
                cmd.client_order_id,
                self.clock.get_time_ns(),
            )?;
            let expected_order = {
                let cache = self.core.cache();
                cmd.client_order_id
                    .and_then(|id| cache.order(&id).map(|order| order.cloned()))
                    .or_else(|| {
                        cache
                            .client_order_id(&venue_order_id)
                            .and_then(|id| cache.order(id).map(|order| order.cloned()))
                    })
            };
            validate_order_response_scope(
                &order,
                &report,
                venue_order_id,
                &instrument,
                expected_order.as_ref(),
            )?;
            let cached_filled = cmd
                .client_order_id
                .and_then(|id| self.core.cache().order(&id).map(|order| order.filled_qty()))
                .or_else(|| {
                    self.core
                        .cache()
                        .client_order_id(&venue_order_id)
                        .and_then(|id| self.core.cache().order(id).map(|order| order.filled_qty()))
                })
                .unwrap_or_else(|| Quantity::zero(size_prec));
            let tracked_filled = self
                .fill_tracker
                .get_cumulative_filled(&venue_order_id)
                .unwrap_or_else(|| Quantity::zero(size_prec));
            let local_filled = cached_filled.max(tracked_filled);
            let confirmed_filled = if report.filled_qty > local_filled {
                match fetch_confirmed_fill_reports(
                    &self.http_client,
                    &self.fill_context(),
                    &self.shared_token_instruments,
                    GetTradesParams::default(),
                    Some(instrument_id),
                    self.clock.get_time_ns(),
                    self.config.reconciliation_load_ids(),
                )
                .await
                {
                    Ok(fills) => confirmed_filled_quantities(&fills)?
                        .get(&venue_order_id)
                        .copied(),
                    Err(e) => {
                        log::warn!(
                            "Failed to fetch confirmed fills for order {venue_order_id}: {e}"
                        );
                        None
                    }
                }
            } else {
                None
            };
            cap_order_report_filled_qty(
                &mut report,
                cached_filled,
                tracked_filled,
                confirmed_filled,
            )?;
            return Ok(Some(report));
        }

        self.recover_terminal_status_from_trades(venue_order_id, &instrument, cmd.client_order_id)
            .await
    }

    fn resolve_venue_order_id(
        &self,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> Option<VenueOrderId> {
        venue_order_id
            .or_else(|| client_order_id.and_then(|id| self.order_identities.venue_order_id(&id)))
    }

    pub(super) async fn generate_order_status_reports_impl(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let params = crate::http::query::GetOrdersParams::default();
        let orders = self
            .http_client
            .get_orders(params)
            .await
            .context("failed to fetch orders")?;

        let (mut reports, _) = super::reconciliation::build_order_reports_from_orders(
            &orders,
            &self.shared_token_instruments,
            self.core.account_id,
            cmd.instrument_id,
            self.clock.get_time_ns(),
            self.config.reconciliation_load_ids(),
        )?;

        let needs_confirmed_fills = reports.iter().any(|report| {
            let cached_filled = report
                .client_order_id
                .and_then(|id| self.core.cache().order(&id).map(|order| order.filled_qty()))
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            report.filled_qty > cached_filled
        });
        let confirmed_fills = if needs_confirmed_fills {
            match fetch_confirmed_fill_reports(
                &self.http_client,
                &self.fill_context(),
                &self.shared_token_instruments,
                GetTradesParams::default(),
                cmd.instrument_id,
                self.clock.get_time_ns(),
                self.config.reconciliation_load_ids(),
            )
            .await
            {
                Ok(fills) => confirmed_filled_quantities(&fills)?,
                Err(e) => {
                    log::warn!("Failed to fetch confirmed fills for open-order check: {e}");
                    Default::default()
                }
            }
        } else {
            Default::default()
        };

        for report in &mut reports {
            let cached_filled = report
                .client_order_id
                .and_then(|id| self.core.cache().order(&id).map(|order| order.filled_qty()))
                .or_else(|| {
                    self.core
                        .cache()
                        .client_order_id(&report.venue_order_id)
                        .and_then(|id| self.core.cache().order(id).map(|order| order.filled_qty()))
                })
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            let tracked_filled = self
                .fill_tracker
                .get_cumulative_filled(&report.venue_order_id)
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            cap_order_report_filled_qty(
                report,
                cached_filled,
                tracked_filled,
                confirmed_fills.get(&report.venue_order_id).copied(),
            )?;
        }

        let reports = if cmd.open_only {
            reports
                .into_iter()
                .filter(|r| r.order_status.is_open())
                .collect()
        } else {
            reports
        };

        log::debug!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    pub(super) async fn generate_fill_reports_impl(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let trades = self
            .http_client
            .get_trades(super::reconciliation::trades_params_for_window(
                cmd.start, cmd.end,
            ))
            .await
            .context("failed to fetch trades")?;

        let ctx = self.fill_context();
        let trades = trades_in_window(
            trades,
            cmd.start,
            cmd.end,
            &ctx,
            &self.shared_token_instruments,
            cmd.instrument_id,
            cmd.venue_order_id,
            self.config.reconciliation_load_ids(),
        )?;
        let (mut reports, _) = build_fill_reports_from_trades(
            &trades,
            &ctx,
            &self.shared_token_instruments,
            cmd.instrument_id,
            self.clock.get_time_ns(),
            self.config.reconciliation_load_ids(),
        )?;

        self.fill_tracker.snap_fill_reports(&mut reports);

        let reports = apply_fill_filters(reports, cmd.venue_order_id, cmd.start, cmd.end);

        log::debug!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    pub(super) async fn generate_position_status_reports_impl(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let ctx = self.fill_context();
        let positions = self
            .data_api_client
            .get_positions(ctx.user_address)
            .await
            .context("failed to fetch positions from Data API")?;

        let ts_now = self.clock.get_time_ns();
        let reports = build_position_reports_scoped(
            &positions,
            &self.shared_token_instruments,
            self.core.account_id,
            cmd.instrument_id,
            self.config.reconciliation_load_ids(),
            ts_now,
        )?;

        log::debug!("Generated {} position status reports", reports.len());
        Ok(reports)
    }

    pub(super) async fn generate_mass_status_impl(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ctx = self.fill_context();
        super::reconciliation::generate_mass_status(
            &self.http_client,
            &self.data_api_client,
            &self.shared_token_instruments,
            &self.fill_tracker,
            &ctx,
            self.core.client_id,
            self.core.venue,
            lookback_mins,
            self.config.reconciliation_load_ids(),
        )
        .await
    }
}

#[expect(clippy::too_many_arguments)]
fn trades_in_window(
    trades: Vec<PolymarketTradeReport>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    venue_order_filter: Option<VenueOrderId>,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<Vec<PolymarketTradeReport>> {
    let mut in_window = Vec::with_capacity(trades.len());
    for trade in trades {
        if !confirmed_trade_in_static_scope(
            &trade,
            ctx,
            instruments,
            instrument_filter,
            venue_order_filter,
            load_ids,
        )? {
            continue;
        }

        if start.is_none() && end.is_none() {
            in_window.push(trade);
            continue;
        }

        let ts_event = parse_match_time(&trade.match_time, "fill match_time")
            .with_context(|| format!("invalid match_time for confirmed trade {}", trade.id))?;

        if start.is_none_or(|value| ts_event >= value) && end.is_none_or(|value| ts_event <= value)
        {
            in_window.push(trade);
        }
    }

    Ok(in_window)
}

async fn fetch_confirmed_fill_reports(
    http_client: &PolymarketClobHttpClient,
    ctx: &FillContext<'_>,
    token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    params: GetTradesParams,
    instrument_id: Option<InstrumentId>,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<Vec<FillReport>> {
    let trades = http_client
        .get_trades(params)
        .await
        .context("failed to fetch confirmed trades")?;
    let (reports, _) = build_fill_reports_from_trades(
        &trades,
        ctx,
        token_instruments,
        instrument_id,
        ts_init,
        load_ids,
    )?;
    Ok(reports)
}

pub(super) fn validate_order_response_scope(
    order: &PolymarketOpenOrder,
    report: &OrderStatusReport,
    requested_venue_order_id: VenueOrderId,
    instrument: &InstrumentAny,
    expected_order: Option<&OrderAny>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        report.venue_order_id == requested_venue_order_id,
        "returned order ID {} does not match requested order ID {requested_venue_order_id}",
        report.venue_order_id,
    );
    anyhow::ensure!(
        report.instrument_id == instrument.id(),
        "returned instrument {} does not match requested instrument {}",
        report.instrument_id,
        instrument.id(),
    );

    if let Some(expected_order) = expected_order {
        if let Some(expected_venue_order_id) = expected_order.venue_order_id() {
            anyhow::ensure!(
                expected_venue_order_id == requested_venue_order_id,
                "requested order ID {requested_venue_order_id} does not match tracked order ID {expected_venue_order_id}",
            );
        }
        anyhow::ensure!(
            expected_order.instrument_id() == instrument.id(),
            "tracked order instrument {} does not match requested instrument {}",
            expected_order.instrument_id(),
            instrument.id(),
        );
        anyhow::ensure!(
            report.order_side == expected_order.order_side(),
            "returned order side {} does not match tracked order side {}",
            report.order_side,
            expected_order.order_side(),
        );
        let provider_tif = TimeInForce::from(order.order_type);
        anyhow::ensure!(
            provider_tif == expected_order.time_in_force(),
            "returned order time in force {provider_tif} does not match tracked order time in force {}",
            expected_order.time_in_force(),
        );
    }

    Ok(())
}

fn pending_trade_matches_known_order(
    trade: &PolymarketTradeReport,
    venue_order_id: VenueOrderId,
    instrument: &InstrumentAny,
    expected_side: Option<OrderSide>,
) -> anyhow::Result<bool> {
    let target = venue_order_id.as_str();
    let taker_matches = trade.taker_order_id == target;
    let maker_matches = trade
        .maker_orders
        .iter()
        .any(|order| order.order_id == target);

    if !taker_matches && !maker_matches {
        return Ok(false);
    }

    if taker_matches {
        ensure_instrument_binding(
            instrument,
            trade.market.as_str(),
            trade.asset_id.as_str(),
            Some(trade.outcome.as_str()),
            "pending taker trade",
        )?;

        if let Some(expected_side) = expected_side {
            let provider_side = OrderSide::from(trade.side);
            anyhow::ensure!(
                provider_side == expected_side,
                "pending taker side {provider_side} does not match tracked order side {expected_side}",
            );
        }
    }

    for maker_order in trade
        .maker_orders
        .iter()
        .filter(|order| order.order_id == target)
    {
        ensure_instrument_binding(
            instrument,
            trade.market.as_str(),
            maker_order.asset_id.as_str(),
            Some(maker_order.outcome.as_str()),
            "pending maker trade",
        )?;

        if let Some(expected_side) = expected_side {
            let provider_side = OrderSide::from(maker_order.side.with_context(|| {
                format!(
                    "pending maker order {} is missing its side",
                    maker_order.order_id
                )
            })?);
            anyhow::ensure!(
                provider_side == expected_side,
                "pending maker side {provider_side} does not match tracked order side {expected_side}",
            );
        }
    }

    Ok(true)
}

pub(crate) fn get_pusd_currency() -> Currency {
    Currency::pUSD()
}

pub(super) async fn fetch_and_emit_account_state(
    http_client: &PolymarketClobHttpClient,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    signature_type: SignatureType,
) -> anyhow::Result<()> {
    let params = GetBalanceAllowanceParams {
        asset_type: Some(crate::http::query::AssetType::Collateral),
        signature_type: Some(signature_type),
        ..Default::default()
    };

    let balance = http_client
        .get_balance(params)
        .await
        .context("failed to fetch balance")?;

    let pusd = get_pusd_currency();
    let account_balance =
        parse_balance_allowance(balance, pusd).context("failed to parse balance")?;

    let ts_event = clock.get_time_ns();
    log::debug!(
        "Account state updated: balance={} pUSD",
        account_balance.total
    );
    emitter.emit_account_state(vec![account_balance], vec![], true, ts_event, None);
    Ok(())
}

pub(super) async fn fetch_collateral_balance_pusd(
    http_client: &PolymarketClobHttpClient,
    signature_type: SignatureType,
) -> anyhow::Result<Decimal> {
    let params = GetBalanceAllowanceParams {
        asset_type: Some(crate::http::query::AssetType::Collateral),
        signature_type: Some(signature_type),
        ..Default::default()
    };

    let balance = http_client
        .get_balance(params)
        .await
        .context("failed to fetch balance")?;

    let usdc_scale = Decimal::from(1_000_000u32);
    Ok(balance / usdc_scale)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::ioc_dust(TimeInForce::Ioc, "5.202910", "5.202897", OrderStatus::Canceled)]
    #[case::ioc_partial(TimeInForce::Ioc, "30", "20", OrderStatus::Canceled)]
    #[case::fok_dust(TimeInForce::Fok, "5.202910", "5.202897", OrderStatus::Filled)]
    #[case::gtc_dust(TimeInForce::Gtc, "5.202910", "5.202897", OrderStatus::Filled)]
    fn test_recovered_terminal_order_status(
        #[case] time_in_force: TimeInForce,
        #[case] quantity: &str,
        #[case] filled_qty: &str,
        #[case] expected: OrderStatus,
    ) {
        assert_eq!(
            recovered_terminal_order_status(
                time_in_force,
                Quantity::from(quantity),
                Quantity::from(filled_qty),
            ),
            expected
        );
    }
}

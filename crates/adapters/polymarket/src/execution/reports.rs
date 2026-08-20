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

use std::collections::{HashMap, HashSet};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_common::messages::execution::{
    GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
    GeneratePositionStatusReports, QueryAccount, QueryOrder,
};
use nautilus_core::{UnixNanos, collections::AtomicMap, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderFilled},
    identifiers::{ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    PolymarketExecutionClient,
    identity::OrderIdentity,
    lifecycle::restored_fill_growth_policy,
    order_fill_tracker::{
        FillFingerprint, FillReplayFingerprint, OrderFillTrackerMap, snap_fill_qty_for_policy,
    },
    parse::{parse_balance_allowance, parse_order_status_report, recovered_terminal_order_status},
    pending::PendingSubmitTracker,
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
    fn known_order_identity(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
    ) -> Option<OrderIdentity> {
        self.order_identities.get(&venue_order_id).or_else(|| {
            let cache = self.core.cache();
            client_order_id
                .or_else(|| cache.client_order_id(&venue_order_id).copied())
                .and_then(|id| cache.order(&id))
                .map(|order| OrderIdentity::from_order(&order))
        })
    }

    fn validate_known_order_report(
        &self,
        report: &mut OrderStatusReport,
        expected_client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<Option<OrderIdentity>> {
        let identity = self.known_order_identity(
            report.venue_order_id,
            expected_client_order_id.or(report.client_order_id),
        );
        if let Some(identity) = identity {
            identity.validate_order_report(
                report,
                report.venue_order_id,
                expected_client_order_id,
            )?;
            report
                .client_order_id
                .get_or_insert(identity.client_order_id);
        }
        Ok(identity)
    }

    fn validate_known_order_reports(
        &self,
        reports: &mut [OrderStatusReport],
    ) -> anyhow::Result<()> {
        for report in reports {
            self.validate_known_order_report(report, report.client_order_id)?;
        }
        Ok(())
    }

    fn validated_confirmed_fills(
        &self,
        reports: &[FillReport],
    ) -> anyhow::Result<ValidatedConfirmedFills> {
        let mut cached_orders = HashMap::new();
        for report in reports {
            let venue_order_id = report.venue_order_id;
            let Some(identity) = self.known_order_identity(venue_order_id, None) else {
                continue;
            };
            anyhow::ensure!(
                report.instrument_id == identity.instrument_id,
                "confirmed fill instrument {} does not match tracked instrument {} for order {venue_order_id}",
                report.instrument_id,
                identity.instrument_id,
            );
            anyhow::ensure!(
                report.order_side == identity.order_side,
                "confirmed fill side {} does not match tracked side {} for order {venue_order_id}",
                report.order_side,
                identity.order_side,
            );
            if let Some(client_order_id) = report.client_order_id {
                anyhow::ensure!(
                    client_order_id == identity.client_order_id,
                    "confirmed fill client order ID {client_order_id} does not match tracked client order ID {}",
                    identity.client_order_id,
                );
            }
            if let Some(order) = self.core.cache().order(&identity.client_order_id) {
                cached_orders
                    .entry(venue_order_id)
                    .or_insert_with(|| order.cloned());
            }
        }
        let mut validated = validate_confirmed_fill_evidence(
            &self.fill_tracker,
            &self.pending_submits,
            &cached_orders,
            reports,
        )?;
        for report in &mut validated.normalized_reports {
            if let Some(identity) = self.known_order_identity(report.venue_order_id, None) {
                report
                    .client_order_id
                    .get_or_insert(identity.client_order_id);
            }
        }
        Ok(validated)
    }

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
                cached.venue_order_id() == Some(venue_order_id),
                "tracked venue order {:?} does not match requested recovery order {venue_order_id}",
                cached.venue_order_id(),
            );
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
        let cached_ts_accepted = cached.as_ref().and_then(|order| order.ts_accepted());

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
                cached_ts_accepted.unwrap_or(ts_init),
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

        let mut scoped_confirmed_trades = Vec::new();
        for trade in &trades {
            if confirmed_trade_in_static_scope(
                trade,
                &ctx,
                &self.shared_token_instruments,
                Some(instrument_id),
                Some(venue_order_id),
                self.config.reconciliation_load_ids(),
            )? {
                scoped_confirmed_trades.push(trade.clone());
            }
        }
        let (mut order_fills, discards) = build_fill_reports_from_trades(
            &scoped_confirmed_trades,
            &ctx,
            &self.shared_token_instruments,
            Some(instrument_id),
            ts_init,
            self.config.reconciliation_load_ids(),
        )?;
        discards.ensure_complete("terminal order recovery")?;
        order_fills.retain(|f| f.venue_order_id == venue_order_id);
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
                cached_ts_accepted.unwrap_or(ts_init),
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

        let confirmed_fills = self.validated_confirmed_fills(&order_fills)?;
        let order_fills = &confirmed_fills.normalized_reports;
        let total_filled_dec = confirmed_fills
            .quantities
            .get(&venue_order_id)
            .context("recovered fills missing confirmed aggregate")?
            .as_decimal();
        let avg_px = confirmed_fills
            .weighted_average_prices
            .get(&venue_order_id)
            .copied();
        let raw_filled_qty =
            non_negative_quantity(total_filled_dec, size_prec, "recovered filled quantity")?;
        let order_side = cached_side.unwrap_or(order_fills[0].order_side);
        let ts_event = order_fills
            .iter()
            .map(|f| f.ts_event)
            .max()
            .unwrap_or(ts_init);
        let ts_accepted = cached_ts_accepted.unwrap_or_else(|| {
            order_fills
                .iter()
                .map(|fill| fill.ts_event)
                .min()
                .unwrap_or(ts_init)
        });

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
            ts_accepted,
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
        let local_filled = cached_filled.max(tracked_filled);
        anyhow::ensure!(
            raw_filled_qty == local_filled,
            "terminal recovery for order {venue_order_id} requires canonical fills to be applied first: local={local_filled}, confirmed={raw_filled_qty}",
        );
        cap_order_report_filled_qty(&mut report, cached_filled, tracked_filled, None)?;

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
        let requested_venue_order_id = venue_order_id;
        let venue_order_id = requested_venue_order_id.to_string();

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
        let cached_orders = expected_order
            .as_ref()
            .map(|order| (requested_venue_order_id, order.clone()))
            .into_iter()
            .collect::<HashMap<_, _>>();

        let http_client = self.http_client.clone();
        let fill_tracker = self.fill_tracker.clone();
        let pending_submits = self.pending_submits.clone();
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
                    let confirmed_fills = if report.filled_qty > local_filled {
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
                            ConfirmedFillLookup {
                                params: GetTradesParams::default(),
                                instrument_id: Some(instrument_id),
                                venue_order_id: Some(venue_order_id),
                                ts_init: clock.get_time_ns(),
                                load_ids: load_ids.as_deref(),
                            },
                        )
                        .await?
                        {
                            Some(fills) => {
                                validate_confirmed_fill_evidence(
                                    &fill_tracker,
                                    &pending_submits,
                                    &cached_orders,
                                    &fills,
                                )?
                            }
                            None => anyhow::bail!(
                                "order {venue_order_id} reports filled quantity {} above applied quantity {local_filled}, but canonical fills are unavailable",
                                report.filled_qty,
                            ),
                        }
                    } else {
                        ValidatedConfirmedFills::default()
                    };
                    let confirmed_filled = confirmed_fills
                        .quantities
                        .get(&venue_order_id)
                        .map(|quantity| quantity.as_decimal());
                    cap_order_report_filled_qty(
                        &mut report,
                        cached_filled,
                        tracked_filled,
                        confirmed_filled,
                    )?;
                    if confirmed_fills.normalized_reports.is_empty() {
                        emitter.send_order_status_report(report);
                    } else {
                        emitter.send_order_with_fills(report, confirmed_fills.normalized_reports);
                    }
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
            let known_identity =
                self.validate_known_order_report(&mut report, cmd.client_order_id)?;
            let expected_order = {
                let cache = self.core.cache();
                known_identity
                    .map(|identity| identity.client_order_id)
                    .or(cmd.client_order_id)
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
            if report.filled_qty > local_filled {
                match fetch_confirmed_fill_reports(
                    &self.http_client,
                    &self.fill_context(),
                    &self.shared_token_instruments,
                    ConfirmedFillLookup {
                        params: GetTradesParams::default(),
                        instrument_id: Some(instrument_id),
                        venue_order_id: Some(venue_order_id),
                        ts_init: self.clock.get_time_ns(),
                        load_ids: self.config.reconciliation_load_ids(),
                    },
                )
                .await?
                {
                    Some(fills) => {
                        self.validated_confirmed_fills(&fills)?;
                    }
                    None => anyhow::bail!(
                        "order {venue_order_id} reports filled quantity {} above applied quantity {local_filled}, but canonical fills are unavailable",
                        report.filled_qty,
                    ),
                }
                anyhow::bail!(
                    "order {venue_order_id} reports filled quantity {} above applied quantity {local_filled}; canonical fills must be reconciled first",
                    report.filled_qty,
                );
            }
            cap_order_report_filled_qty(&mut report, cached_filled, tracked_filled, None)?;
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
        self.validate_known_order_reports(&mut reports)?;

        let needs_confirmed_fills = reports.iter().any(|report| {
            let cached_filled = report
                .client_order_id
                .and_then(|id| self.core.cache().order(&id).map(|order| order.filled_qty()))
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            report.filled_qty > cached_filled
        });
        if needs_confirmed_fills {
            match fetch_confirmed_fill_reports(
                &self.http_client,
                &self.fill_context(),
                &self.shared_token_instruments,
                ConfirmedFillLookup {
                    params: GetTradesParams::default(),
                    instrument_id: cmd.instrument_id,
                    venue_order_id: None,
                    ts_init: self.clock.get_time_ns(),
                    load_ids: self.config.reconciliation_load_ids(),
                },
            )
            .await?
            {
                Some(fills) => {
                    self.validated_confirmed_fills(&fills)?;
                }
                None => anyhow::bail!(
                    "order reports contain filled quantity above applied authority, but canonical fills are unavailable",
                ),
            }
        }

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
            let local_filled = cached_filled.max(tracked_filled);
            anyhow::ensure!(
                report.filled_qty <= local_filled,
                "order {} reports filled quantity {} above applied quantity {local_filled}; canonical fills must be reconciled first",
                report.venue_order_id,
                report.filled_qty,
            );
            cap_order_report_filled_qty(report, cached_filled, tracked_filled, None)?;
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
        let (reports, discards) = build_fill_reports_from_trades(
            &trades,
            &ctx,
            &self.shared_token_instruments,
            cmd.instrument_id,
            self.clock.get_time_ns(),
            self.config.reconciliation_load_ids(),
        )?;
        discards.ensure_complete("fill report generation")?;

        let validated = self.validated_confirmed_fills(&reports)?;
        let reports = apply_fill_filters(
            validated.normalized_reports,
            cmd.venue_order_id,
            cmd.start,
            cmd.end,
        );

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
        let generated = super::reconciliation::generate_mass_status(
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
        .await?;

        let Some(generated) = generated else {
            return Ok(None);
        };
        let (validated_fills, mut order_reports) = {
            let status = &generated.status;
            let mut order_reports = status.order_reports().into_values().collect::<Vec<_>>();
            self.validate_known_order_reports(&mut order_reports)?;
            (
                self.validated_confirmed_fills(&generated.provider_fill_reports)?,
                order_reports,
            )
        };
        for report in &mut order_reports {
            let cached_filled = {
                let cache = self.core.cache();
                report
                    .client_order_id
                    .and_then(|id| cache.order(&id).map(|order| order.filled_qty()))
                    .or_else(|| {
                        cache
                            .client_order_id(&report.venue_order_id)
                            .and_then(|id| cache.order(id).map(|order| order.filled_qty()))
                    })
                    .unwrap_or_else(|| Quantity::zero(report.quantity.precision))
            };
            let tracked_filled = self
                .fill_tracker
                .get_cumulative_filled(&report.venue_order_id)
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            cap_order_report_filled_qty(
                report,
                cached_filled,
                tracked_filled,
                validated_fills
                    .quantities
                    .get(&report.venue_order_id)
                    .map(|quantity| quantity.as_decimal()),
            )?;
        }
        let mut status = generated.status;
        status.add_order_reports(order_reports);
        status.add_fill_reports(validated_fills.normalized_reports);
        Ok(Some(status))
    }
}

#[derive(Default)]
struct ValidatedConfirmedFills {
    quantities: AHashMap<VenueOrderId, Quantity>,
    weighted_average_prices: AHashMap<VenueOrderId, Decimal>,
    normalized_reports: Vec<FillReport>,
}

fn insert_fill_economics(
    fills: &mut HashMap<(VenueOrderId, TradeId), (Quantity, Price)>,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    last_qty: Quantity,
    last_px: Price,
) -> anyhow::Result<()> {
    if let Some((expected_qty, expected_px)) = fills.get(&(venue_order_id, trade_id)) {
        anyhow::ensure!(
            *expected_qty == last_qty && *expected_px == last_px,
            "different admitted fill economics for order {venue_order_id} trade {trade_id}: expected {expected_qty}@{expected_px}, received {last_qty}@{last_px}",
        );
    } else {
        fills.insert((venue_order_id, trade_id), (last_qty, last_px));
    }
    Ok(())
}

fn validate_confirmed_fill_evidence(
    fill_tracker: &OrderFillTrackerMap,
    pending_submits: &PendingSubmitTracker,
    cached_orders: &HashMap<VenueOrderId, OrderAny>,
    provider_reports: &[FillReport],
) -> anyhow::Result<ValidatedConfirmedFills> {
    let mut reports = provider_reports.to_vec();
    fill_tracker.snap_fill_reports(&mut reports);
    for report in &mut reports {
        let Some(order) = cached_orders.get(&report.venue_order_id) else {
            continue;
        };
        anyhow::ensure!(
            report.instrument_id == order.instrument_id(),
            "confirmed fill instrument {} does not match cached instrument {} for order {}",
            report.instrument_id,
            order.instrument_id(),
            report.venue_order_id,
        );
        anyhow::ensure!(
            report.order_side == order.order_side(),
            "confirmed fill side {} does not match cached side {} for order {}",
            report.order_side,
            order.order_side(),
            report.venue_order_id,
        );
        if !fill_tracker.contains(&report.venue_order_id) {
            report.last_qty = snap_fill_qty_for_policy(
                order.quantity(),
                restored_fill_growth_policy(order),
                report.last_qty,
            );
        }
    }
    let mut confirmed_filled = confirmed_filled_quantities(&reports)?;
    let mut returned_fills = HashMap::<(VenueOrderId, TradeId), FillFingerprint>::new();
    for provider_report in provider_reports {
        let key = (provider_report.venue_order_id, provider_report.trade_id);
        let fingerprint = FillFingerprint::from_report(provider_report);
        if let Some(existing) = returned_fills.get(&key) {
            existing.ensure_equal(&fingerprint, provider_report.venue_order_id)?;
        } else {
            returned_fills.insert(key, fingerprint);
        }
    }
    let mut cached_fills = Vec::<OrderFilled>::new();
    for order in cached_orders.values() {
        let active_trade_ids = order
            .trade_ids()
            .into_iter()
            .copied()
            .collect::<HashSet<_>>();
        for event in order.events() {
            if let OrderEventAny::Filled(fill) = event
                && active_trade_ids.contains(&fill.trade_id)
            {
                cached_fills.push(fill.clone());
            }
        }
    }

    // The tracker owns current-session quantity semantics, including a signed quote budget.
    // It returns the exact orders validated under its lock, leaving only cache-only orders for
    // the fixed ceiling below.
    let pending_orders = provider_reports
        .iter()
        .filter_map(|report| {
            pending_submits
                .fill_validation_proof(&report.venue_order_id)
                .map(|proof| (report.venue_order_id, proof))
        })
        .collect::<AHashMap<_, _>>();
    let tracker_totals = fill_tracker.validate_confirmed_fills_with_pending(
        &cached_fills,
        provider_reports,
        &reports,
        &pending_orders,
    )?;
    let mut prospective_totals = HashMap::new();
    let mut seen_fills = HashMap::<(VenueOrderId, TradeId), FillReplayFingerprint>::new();
    let mut fill_economics = HashMap::new();
    for fill in &cached_fills {
        let key = (fill.venue_order_id, fill.trade_id);
        let fingerprint = FillReplayFingerprint::from_event(fill)?;
        if let Some(existing) = seen_fills.get(&key) {
            existing.ensure_equal(&fingerprint, fill.venue_order_id)?;
        } else {
            seen_fills.insert(key, fingerprint);
        }
        insert_fill_economics(
            &mut fill_economics,
            fill.venue_order_id,
            fill.trade_id,
            fill.last_qty,
            fill.last_px,
        )?;
    }

    let relevant_orders = cached_orders
        .keys()
        .copied()
        .chain(reports.iter().map(|report| report.venue_order_id))
        .collect::<AHashSet<_>>();
    for (venue_order_id, trade_id, last_qty, last_px) in
        fill_tracker.applied_fill_economics(&relevant_orders)
    {
        insert_fill_economics(
            &mut fill_economics,
            venue_order_id,
            trade_id,
            last_qty,
            last_px,
        )?;
    }

    for (venue_order_id, order) in cached_orders {
        if !tracker_totals.contains_key(venue_order_id) {
            prospective_totals.insert(*venue_order_id, order.filled_qty());
        }
    }
    for (provider_report, report) in provider_reports.iter().zip(&reports) {
        let venue_order_id = report.venue_order_id;
        if tracker_totals.contains_key(&venue_order_id) {
            insert_fill_economics(
                &mut fill_economics,
                venue_order_id,
                report.trade_id,
                report.last_qty,
                report.last_px,
            )?;
            continue;
        }

        let Some(order) = cached_orders.get(&venue_order_id) else {
            insert_fill_economics(
                &mut fill_economics,
                venue_order_id,
                report.trade_id,
                report.last_qty,
                report.last_px,
            )?;
            continue;
        };
        let key = (venue_order_id, provider_report.trade_id);
        let fingerprint = FillReplayFingerprint::from_reports(provider_report, report);
        if let Some(existing) = seen_fills.get(&key) {
            existing.ensure_equal(&fingerprint, venue_order_id)?;
            insert_fill_economics(
                &mut fill_economics,
                venue_order_id,
                report.trade_id,
                report.last_qty,
                report.last_px,
            )?;
            continue;
        }
        seen_fills.insert(key, fingerprint);
        insert_fill_economics(
            &mut fill_economics,
            venue_order_id,
            report.trade_id,
            report.last_qty,
            report.last_px,
        )?;

        let total = prospective_totals
            .entry(venue_order_id)
            .or_insert(order.filled_qty());
        *total = total.checked_add(report.last_qty).ok_or_else(|| {
            anyhow::anyhow!(
                "confirmed filled quantity overflow for order {venue_order_id}: {} + {}",
                *total,
                report.last_qty,
            )
        })?;
        anyhow::ensure!(
            *total <= order.quantity(),
            "confirmed fill aggregate {total} exceeds cached quantity {} for order {venue_order_id}",
            order.quantity()
        );
    }
    confirmed_filled.extend(prospective_totals);
    confirmed_filled.extend(tracker_totals);
    let mut weighted_economics = HashMap::<VenueOrderId, (Decimal, Decimal)>::new();
    for ((venue_order_id, _), (last_qty, last_px)) in fill_economics {
        let qty = last_qty.as_decimal();
        let notional = qty
            .checked_mul(last_px.as_decimal())
            .context("fill notional multiplication overflow")?;
        let (total_qty, total_notional) = weighted_economics
            .entry(venue_order_id)
            .or_insert((Decimal::ZERO, Decimal::ZERO));
        *total_qty = total_qty
            .checked_add(qty)
            .context("fill quantity sum overflow")?;
        *total_notional = total_notional
            .checked_add(notional)
            .context("fill notional sum overflow")?;
    }
    let mut weighted_average_prices = AHashMap::new();
    for (venue_order_id, total) in &confirmed_filled {
        if total.is_zero() {
            continue;
        }
        let (economic_qty, economic_notional) = weighted_economics
            .get(venue_order_id)
            .with_context(|| format!("missing fill economics for order {venue_order_id}"))?;
        anyhow::ensure!(
            *economic_qty == total.as_decimal(),
            "fill economics quantity {economic_qty} does not match confirmed aggregate {total} for order {venue_order_id}",
        );
        weighted_average_prices.insert(
            *venue_order_id,
            economic_notional
                .checked_div(*economic_qty)
                .context("weighted average price division failed")?,
        );
    }
    Ok(ValidatedConfirmedFills {
        quantities: confirmed_filled,
        weighted_average_prices,
        normalized_reports: reports,
    })
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

struct ConfirmedFillLookup<'a> {
    params: GetTradesParams,
    instrument_id: Option<InstrumentId>,
    venue_order_id: Option<VenueOrderId>,
    ts_init: UnixNanos,
    load_ids: Option<&'a [InstrumentId]>,
}

async fn fetch_confirmed_fill_reports(
    http_client: &PolymarketClobHttpClient,
    ctx: &FillContext<'_>,
    token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    lookup: ConfirmedFillLookup<'_>,
) -> anyhow::Result<Option<Vec<FillReport>>> {
    let ConfirmedFillLookup {
        params,
        instrument_id,
        venue_order_id,
        ts_init,
        load_ids,
    } = lookup;
    let trades = match http_client.get_trades(params).await {
        Ok(trades) => trades,
        Err(e) if e.is_retryable() => {
            log::warn!("Failed to fetch confirmed trades: {e}");
            return Ok(None);
        }
        Err(e) => return Err(e).context("invalid confirmed-trade response"),
    };
    let mut scoped_trades = Vec::new();
    for trade in trades {
        if confirmed_trade_in_static_scope(
            &trade,
            ctx,
            token_instruments,
            instrument_id,
            venue_order_id,
            load_ids,
        )? {
            scoped_trades.push(trade);
        }
    }
    let (mut reports, discards) = build_fill_reports_from_trades(
        &scoped_trades,
        ctx,
        token_instruments,
        instrument_id,
        ts_init,
        load_ids,
    )?;
    discards.ensure_complete("confirmed fill lookup")?;
    if let Some(venue_order_id) = venue_order_id {
        reports.retain(|report| report.venue_order_id == venue_order_id);
    }
    Ok(Some(reports))
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

pub(crate) fn pending_trade_matches_known_order(
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

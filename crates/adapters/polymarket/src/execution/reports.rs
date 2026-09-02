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
        parse_balance_allowance, recovered_terminal_order_status, sum_filled_quantity,
        weighted_average_price,
    },
    reconciliation::{
        FillContext, FillReportScope, TargetOrderReportScope, apply_fill_time_filters,
        build_fill_reports_from_trades, build_reconciliation_position_reports,
        build_target_order_report, cap_order_report_filled_qty, confirmed_filled_quantities,
        normalize_terminal_order_report_quantity,
    },
};
use crate::{
    common::enums::SignatureType,
    http::{
        clob::PolymarketClobHttpClient,
        query::{GetBalanceAllowanceParams, GetTradesParams},
    },
};

#[derive(Clone)]
struct TargetOrderAuthority {
    client_order_id: Option<ClientOrderId>,
    cached_order: Option<OrderAny>,
    instrument_id: Option<InstrumentId>,
    order_side: Option<OrderSide>,
}

impl TargetOrderAuthority {
    fn require_cached_base_limit(&self, venue_order_id: VenueOrderId) -> anyhow::Result<&OrderAny> {
        let client_order_id = self.client_order_id.with_context(|| {
            format!("venue order {venue_order_id} has no known client association")
        })?;
        let cached_order = self.cached_order.as_ref().with_context(|| {
            format!("client-bound order report requires cached order {client_order_id}")
        })?;
        anyhow::ensure!(
            cached_order.order_type() == OrderType::Limit && !cached_order.is_quote_quantity(),
            "client-bound Polymarket reports require a cached base-denominated Limit order",
        );
        Ok(cached_order)
    }
}

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
            pusd: get_pusd_currency(),
            clock: self.clock,
        }
    }

    fn resolve_target_order_authority(
        &self,
        explicit_client_order_id: Option<ClientOrderId>,
        venue_order_id: VenueOrderId,
        requested_instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<TargetOrderAuthority> {
        let identity = self.order_identities.get(&venue_order_id);
        let cached_client_order_id = self.core.cache().client_order_id(&venue_order_id).copied();

        let mut client_order_id = explicit_client_order_id;
        for candidate in [
            identity.map(|value| value.client_order_id),
            cached_client_order_id,
        ]
        .into_iter()
        .flatten()
        {
            if let Some(known) = client_order_id {
                anyhow::ensure!(
                    candidate == known,
                    "venue order {venue_order_id} has contradictory client associations {known} and {candidate}",
                );
            } else {
                client_order_id = Some(candidate);
            }
        }

        if let Some(client_order_id) = client_order_id
            && let Some(registered_venue_order_id) =
                self.order_identities.venue_order_id(&client_order_id)
        {
            anyhow::ensure!(
                registered_venue_order_id == venue_order_id,
                "client order {client_order_id} is registered to venue order {registered_venue_order_id}, not requested venue order {venue_order_id}",
            );
        }

        let cached_order = client_order_id.and_then(|value| self.core.cache().order_owned(&value));
        if let Some(cached_order) = cached_order.as_ref() {
            let cached_venue_order_id = cached_order.venue_order_id();
            if let Some(cached_venue_order_id) = cached_venue_order_id {
                anyhow::ensure!(
                    cached_venue_order_id == venue_order_id,
                    "cached client order {} is associated with venue order {cached_venue_order_id}, not requested venue order {venue_order_id}",
                    cached_order.client_order_id(),
                );
            } else {
                anyhow::ensure!(
                    cached_client_order_id == client_order_id
                        || self
                            .order_identities
                            .venue_order_id(&cached_order.client_order_id())
                            == Some(venue_order_id),
                    "cached client order {} has no association with requested venue order {venue_order_id}",
                    cached_order.client_order_id(),
                );
            }

            if let Some(requested_instrument_id) = requested_instrument_id {
                anyhow::ensure!(
                    cached_order.instrument_id() == requested_instrument_id,
                    "cached order instrument {} does not match requested instrument {requested_instrument_id}",
                    cached_order.instrument_id(),
                );
            }
        }

        if let Some(identity) = identity {
            if let Some(requested_instrument_id) = requested_instrument_id {
                anyhow::ensure!(
                    identity.instrument_id == requested_instrument_id,
                    "registered order instrument {} does not match requested instrument {requested_instrument_id}",
                    identity.instrument_id,
                );
            }

            if let Some(cached_order) = cached_order.as_ref() {
                anyhow::ensure!(
                    identity.client_order_id == cached_order.client_order_id()
                        && identity.instrument_id == cached_order.instrument_id()
                        && identity.order_side == cached_order.order_side()
                        && identity.order_type == cached_order.order_type()
                        && identity.time_in_force == cached_order.time_in_force(),
                    "registered order identity for {venue_order_id} contradicts cached order {}",
                    cached_order.client_order_id(),
                );
            }
        }

        Ok(TargetOrderAuthority {
            client_order_id,
            instrument_id: identity
                .map(|value| value.instrument_id)
                .or_else(|| cached_order.as_ref().map(Order::instrument_id)),
            order_side: identity
                .map(|value| value.order_side)
                .or_else(|| cached_order.as_ref().map(|order| order.order_side())),
            cached_order,
        })
    }

    async fn recover_terminal_status_from_trades(
        &self,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        authority: TargetOrderAuthority,
        size_prec: u8,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let ts_init = self.clock.get_time_ns();
        let ctx = self.fill_context();

        let trades = self
            .http_client
            .get_trades(GetTradesParams::default())
            .await
            .context("failed to fetch trades for order recovery")?;

        let resolved_client_order_id = authority.client_order_id;
        let cached = authority.cached_order;
        let cached_quantity = cached.as_ref().map(Order::quantity);
        let cached_order_type = cached.as_ref().map_or(OrderType::Limit, Order::order_type);
        let cached_tif = cached
            .as_ref()
            .map_or(TimeInForce::Gtc, Order::time_in_force);
        let cached_price = cached.as_ref().and_then(Order::price);
        let cached_side = cached.as_ref().map(|order| order.order_side());
        let expected_order_side = authority.order_side;

        let (mut order_fills, fill_discards) = build_fill_reports_from_trades(
            &trades,
            &ctx,
            &self.shared_token_instruments,
            FillReportScope::new(Some(instrument_id), Some(venue_order_id))
                .with_expected_order_side(expected_order_side),
            ts_init,
            self.config.reconciliation_load_ids(),
            None,
        )?;

        if fill_discards.has_pending_target {
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
                cached.order_side().into(),
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
                cached.order_side().into(),
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

        let total_filled_dec = sum_filled_quantity(&order_fills);
        let avg_px = weighted_average_price(&order_fills, total_filled_dec);
        let raw_filled_qty = Quantity::from_decimal_dp(total_filled_dec, size_prec)
            .unwrap_or_else(|_| Quantity::zero(size_prec));
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
            order_side.into(),
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
        normalize_terminal_order_report_quantity(&mut report);

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

        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let authority = match self.resolve_target_order_authority(
            Some(client_order_id),
            venue_order_id,
            Some(instrument_id),
        ) {
            Ok(authority) => authority,
            Err(e) => {
                log::warn!("Cannot query client-bound order {client_order_id}: {e}");
                return;
            }
        };
        let cached_order = match authority.require_cached_base_limit(venue_order_id) {
            Ok(order) => order.clone(),
            Err(e) => {
                log::warn!("Cannot query client-bound order {client_order_id}: {e}");
                return;
            }
        };
        let account_id = self.core.account_id;
        let Some(instrument) = self.core.cache().instrument(&instrument_id).cloned() else {
            log::warn!(
                "Cannot query order {client_order_id}: instrument {instrument_id} not cached"
            );
            return;
        };
        let size_prec = instrument.size_precision();
        let venue_order_id_str = venue_order_id.to_string();

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
        let cached_filled = cached_order.filled_qty();

        self.spawn_task("query_order", async move {
            match http_client.get_order_optional(&venue_order_id_str).await {
                Ok(Some(order)) => {
                    let ctx = FillContext {
                        account_id,
                        user_address: &user_address,
                        api_key: &api_key,
                        pusd: get_pusd_currency(),
                        clock,
                    };
                    let mut report = build_target_order_report(
                        &order,
                        &token_instruments,
                        &ctx,
                        TargetOrderReportScope::new(
                            instrument_id,
                            venue_order_id,
                            Some(client_order_id),
                            Some(&cached_order),
                        ),
                        clock.get_time_ns(),
                    )?;
                    let tracked_filled = fill_tracker
                        .get_cumulative_filled(&venue_order_id)
                        .unwrap_or_else(|| Quantity::zero(size_prec));
                    let local_filled = cached_filled.max(tracked_filled);
                    let confirmed_filled = if report.filled_qty > local_filled {
                        fetch_confirmed_fill_reports(
                            &http_client,
                            &ctx,
                            &token_instruments,
                            GetTradesParams::default(),
                            FillReportScope::new(Some(instrument_id), Some(venue_order_id))
                                .with_expected_order_side(report.order_side),
                            clock.get_time_ns(),
                            load_ids.as_deref(),
                        )
                        .await?
                        .as_deref()
                        .and_then(|fills| {
                            confirmed_filled_quantities(fills)
                                .get(&venue_order_id)
                                .copied()
                        })
                    } else {
                        None
                    };
                    cap_order_report_filled_qty(&mut report, local_filled, confirmed_filled);
                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::warn!("Order {venue_order_id_str} not found (empty response)");
                }
                Err(e) => {
                    log::warn!("Failed to query order {venue_order_id_str}: {e}");
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

        let authority = self.resolve_target_order_authority(
            cmd.client_order_id,
            venue_order_id,
            Some(instrument_id),
        )?;
        let cached_authority = authority
            .client_order_id
            .map(|_| authority.require_cached_base_limit(venue_order_id).cloned())
            .transpose()?;
        let instrument = self
            .core
            .cache()
            .instrument(&instrument_id)
            .cloned()
            .with_context(|| format!("instrument {instrument_id} not cached"))?;
        let size_prec = instrument.size_precision();

        let order = self
            .http_client
            .get_order_optional(venue_order_id.as_str())
            .await
            .context("failed to fetch order")?;

        if let Some(order) = order {
            let mut report = build_target_order_report(
                &order,
                &self.shared_token_instruments,
                &self.fill_context(),
                TargetOrderReportScope::new(
                    instrument_id,
                    venue_order_id,
                    authority.client_order_id,
                    cached_authority.as_ref(),
                ),
                self.clock.get_time_ns(),
            )?;
            let cached_filled = cached_authority
                .as_ref()
                .map_or_else(|| Quantity::zero(size_prec), Order::filled_qty);
            let tracked_filled = self
                .fill_tracker
                .get_cumulative_filled(&venue_order_id)
                .unwrap_or_else(|| Quantity::zero(size_prec));
            let local_filled = cached_filled.max(tracked_filled);
            let confirmed_filled = if report.filled_qty > local_filled {
                fetch_confirmed_fill_reports(
                    &self.http_client,
                    &self.fill_context(),
                    &self.shared_token_instruments,
                    GetTradesParams::default(),
                    FillReportScope::new(Some(instrument_id), Some(venue_order_id))
                        .with_expected_order_side(report.order_side),
                    self.clock.get_time_ns(),
                    self.config.reconciliation_load_ids(),
                )
                .await?
                .as_deref()
                .and_then(|fills| {
                    confirmed_filled_quantities(fills)
                        .get(&venue_order_id)
                        .copied()
                })
            } else {
                None
            };
            cap_order_report_filled_qty(&mut report, local_filled, confirmed_filled);
            return Ok(Some(report));
        }

        self.recover_terminal_status_from_trades(
            venue_order_id,
            instrument_id,
            authority,
            size_prec,
        )
        .await
    }

    fn resolve_venue_order_id(
        &self,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> Option<VenueOrderId> {
        venue_order_id
            .or_else(|| client_order_id.and_then(|id| self.order_identities.venue_order_id(&id)))
            .or_else(|| {
                client_order_id.and_then(|id| {
                    self.core
                        .cache()
                        .order(&id)
                        .and_then(|order| order.venue_order_id())
                })
            })
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

        let ctx = self.fill_context();
        let collection_load_ids = if cmd.instrument_id.is_some() {
            None
        } else {
            self.config.reconciliation_load_ids()
        };
        let (mut reports, _) = super::reconciliation::build_order_reports_from_orders(
            &orders,
            &self.shared_token_instruments,
            &ctx,
            cmd.instrument_id,
            self.clock.get_time_ns(),
            collection_load_ids,
        )?;

        let needs_confirmed_fills = reports.iter().any(|report| {
            let cached_filled = report
                .client_order_id
                .and_then(|id| self.core.cache().order(&id).map(|order| order.filled_qty()))
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            report.filled_qty > cached_filled
        });
        let confirmed_fills = if needs_confirmed_fills {
            match self
                .http_client
                .get_trades(GetTradesParams::default())
                .await
            {
                Ok(trades) => {
                    let (fills, _) = build_fill_reports_from_trades(
                        &trades,
                        &ctx,
                        &self.shared_token_instruments,
                        FillReportScope::new(cmd.instrument_id, None),
                        self.clock.get_time_ns(),
                        collection_load_ids,
                        None,
                    )?;
                    confirmed_filled_quantities(&fills)
                }
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
                cached_filled.max(tracked_filled),
                confirmed_fills.get(&report.venue_order_id).copied(),
            );
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
        let authority = cmd
            .venue_order_id
            .map(|venue_order_id| {
                self.resolve_target_order_authority(None, venue_order_id, cmd.instrument_id)
            })
            .transpose()?;
        let scope_instrument_id = cmd
            .instrument_id
            .or_else(|| authority.as_ref().and_then(|value| value.instrument_id));
        let expected_order_side = authority.as_ref().and_then(|value| value.order_side);
        let collection_load_ids = if cmd.instrument_id.is_some() || cmd.venue_order_id.is_some() {
            None
        } else {
            self.config.reconciliation_load_ids()
        };
        let (mut reports, _) = build_fill_reports_from_trades(
            &trades,
            &ctx,
            &self.shared_token_instruments,
            FillReportScope::new(scope_instrument_id, cmd.venue_order_id)
                .with_expected_order_side(expected_order_side),
            self.clock.get_time_ns(),
            collection_load_ids,
            None,
        )?;

        self.fill_tracker.snap_fill_reports(&mut reports);

        let reports = apply_fill_time_filters(reports, cmd.start, cmd.end);

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
        let reports = build_reconciliation_position_reports(
            &positions,
            self.core.account_id,
            ts_now,
            &self.shared_token_instruments,
            cmd.instrument_id,
            self.config.reconciliation_load_ids(),
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

/// Returns `Ok(None)` only when confirmed trades are unavailable; decoded semantic errors propagate.
async fn fetch_confirmed_fill_reports(
    http_client: &PolymarketClobHttpClient,
    ctx: &FillContext<'_>,
    token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    params: GetTradesParams,
    scope: FillReportScope,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<Option<Vec<FillReport>>> {
    let trades = match http_client.get_trades(params).await {
        Ok(trades) => trades,
        Err(e) => {
            log::warn!("Failed to fetch confirmed fills for {scope:?}: {e}");
            return Ok(None);
        }
    };
    let (reports, _) = build_fill_reports_from_trades(
        &trades,
        ctx,
        token_instruments,
        scope,
        ts_init,
        load_ids,
        None,
    )?;
    Ok(Some(reports))
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
    // Not a full snapshot: `GET /balance-allowance` returns no reservation figure, so
    // reporting it as one would clear the portfolio's locks from open orders
    emitter.emit_account_state(vec![account_balance], vec![], false, ts_event, None);
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

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

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_common::{
    clients::ExecutionReportTask,
    messages::execution::{
        GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GeneratePositionStatusReports, QueryAccount, QueryOrder,
    },
};
use nautilus_core::{
    UnixNanos, collections::AtomicMap, string::secret::SecretString, time::AtomicTime,
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter, execution::context::OrderContext};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Quantity},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    PolymarketExecutionClient,
    collateral::{AppliedBalance, CollateralLocks},
    parse::{
        parse_balance_allowance, recovered_terminal_order_status, sum_filled_quantity,
        weighted_average_price,
    },
    reconciliation::{
        FillContext, FillReportScope, ResolvedBalanceScope, TargetOrderReportScope,
        apply_fill_time_filters, apply_logical_order_quantity, build_fill_reports_from_trades,
        build_reconciliation_position_reports, build_target_order_report,
        cap_order_report_filled_qty, collect_fill_reports_from_trades, filter_unsupported_orders,
        filter_unsupported_positions, instrument_id_from_market_token,
        normalize_terminal_order_report_quantity, order_status_recovery_status,
        order_status_scope_awaits_application, position_in_report_scope, trade_touches_venue_order,
        venue_leg_filled_before_and_quantity, without_order_status_pending_trades,
    },
    responses::confirm_modify_replacement,
    settlement::{SettlementRegistry, registry::CollectedFillReports},
};
use crate::{
    common::enums::{PolymarketSignatureType, PolymarketSignerType},
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        query::{GetBalanceAllowanceParams, GetTradesParams},
    },
    websocket::dispatch::{
        ModifyPromotion, WsDispatchContext, WsDispatchState, apply_uncertain_order_evidence,
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
    pub(super) fn order_status_report_task(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> ExecutionReportTask<Option<OrderStatusReport>> {
        let client = self.report_client();
        let state = self.report_state();
        let request = self
            .settlement
            .ensure_order_status_available(cmd.instrument_id, "order status report")
            .and_then(|()| self.prepare_order_status_report(cmd));
        let instrument_id = cmd.instrument_id;

        ExecutionReportTask::new(
            async move { client.collect_order_status_report(request?).await },
            move |collected| {
                state
                    .settlement
                    .ensure_order_status_available(instrument_id, "order status report")?;
                state.finish_order_status_report(collected)
            },
        )
    }

    pub(super) fn order_status_reports_task(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> Option<ExecutionReportTask<Vec<OrderStatusReport>>> {
        // Pending replacements require the inline path's event-emitting promotion
        if !self
            .ws_dispatch_state
            .lock()
            .pending_modify_promotions()
            .is_empty()
        {
            return None;
        }

        let client = self.report_client();
        let state = self.report_state();
        let command = cmd.clone();
        let collection_command = command.clone();
        let gate = self
            .settlement
            .ensure_order_status_available(cmd.instrument_id, "order status reports");

        let cached_filled = self
            .core
            .cache()
            .orders_refs(
                Some(&self.core.venue),
                cmd.instrument_id.as_ref(),
                None,
                Some(&self.core.account_id),
                None,
            )
            .into_iter()
            .filter_map(|order| {
                order.venue_order_id().map(|id| {
                    let filled = venue_leg_filled_before_and_quantity(
                        &order,
                        id,
                        order.filled_qty().precision,
                    )
                    .map(|(before, _)| order.filled_qty().as_decimal() - before.as_decimal())
                    .ok();
                    (id, filled)
                })
            })
            .collect();

        Some(ExecutionReportTask::new(
            async move {
                gate?;
                client
                    .collect_order_status_reports(&collection_command, cached_filled)
                    .await
            },
            move |(mut reports, fills)| {
                state
                    .settlement
                    .ensure_order_status_available(command.instrument_id, "order status reports")?;
                anyhow::ensure!(
                    state
                        .ws_dispatch_state
                        .lock()
                        .pending_modify_promotions()
                        .is_empty(),
                    "pending Polymarket modification requires inline order reconciliation"
                );
                {
                    let dispatch = state.ws_dispatch_state.lock();
                    reports.retain(|report| {
                        report.order_status != OrderStatus::Canceled
                            || !dispatch.suppress_modify_cancel_reemit(report.venue_order_id)
                    });
                }

                let offsets = state.logical_fill_offsets(&mut reports, &AHashMap::new())?;

                let quantities = if fills.reports.is_empty() {
                    AHashMap::new()
                } else {
                    let cache = state.core.cache();
                    let orders = cache.orders_refs(
                        Some(&state.core.venue),
                        command.instrument_id.as_ref(),
                        None,
                        Some(&state.core.account_id),
                        None,
                    );
                    state.settlement.order_status_filled_quantities(
                        &fills.reports,
                        orders.iter().map(|order| &**order),
                    )?
                };

                let reports = state.finish_order_status_reports(
                    reports,
                    &quantities,
                    &offsets,
                    &AHashMap::new(),
                    &command,
                );
                state.settlement.accept_fill_reports(&fills)?;
                Ok(reports)
            },
        ))
    }

    pub(super) fn fill_reports_task(
        &self,
        cmd: &GenerateFillReports,
    ) -> ExecutionReportTask<Vec<FillReport>> {
        let client = self.report_client();
        let settlement = self.settlement.clone();
        let command = cmd.clone();
        let scope = fill_report_gate(&self.settlement, cmd)
            .and_then(|()| self.prepare_fill_report_scope(cmd));

        ExecutionReportTask::new(
            async move { client.collect_fill_reports(&command, scope?).await },
            {
                let command = cmd.clone();
                move |reports| {
                    fill_report_gate(&settlement, &command)?;
                    settlement.accept_fill_reports(&reports)?;
                    Ok(reports.reports)
                }
            },
        )
    }

    pub(super) fn position_status_reports_task(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> ExecutionReportTask<Vec<PositionStatusReport>> {
        let client = self.report_client();
        let state = self.report_state();
        let command = cmd.clone();
        let instrument_id = cmd.instrument_id;
        let scope = self.resolved_balance_scope();
        let gate = self
            .settlement
            .ensure_resolved(instrument_id, "position status reports");

        ExecutionReportTask::new(
            async move {
                gate?;
                client
                    .collect_position_status_reports(&command, scope)
                    .await
            },
            move |collected| {
                state
                    .settlement
                    .ensure_resolved(instrument_id, "position status reports")?;
                state.finish_position_status_reports(collected)
            },
        )
    }

    pub(super) fn fill_context(&self) -> FillContext<'_> {
        let signer_type = self.config.signer_type;
        let user_address = self
            .secrets
            .funder
            .as_deref()
            .unwrap_or(&self.secrets.address);
        FillContext {
            signer_type,
            account_id: self.core.account_id,
            user_address,
            api_key: self.secrets.credential.api_key_str(),
            pusd: get_pusd_currency(),
            clock: self.clock,
            settlement: self.settlement.clone(),
        }
    }

    fn resolve_target_order_authority(
        &self,
        explicit_client_order_id: Option<ClientOrderId>,
        venue_order_id: VenueOrderId,
        requested_instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<TargetOrderAuthority> {
        let context = self.order_contexts.get(&venue_order_id);
        let cached_client_order_id = self.core.cache().client_order_id(&venue_order_id).copied();

        let mut client_order_id = explicit_client_order_id;
        for candidate in [
            context.map(|value| value.identity.client_order_id),
            cached_client_order_id,
            self.pending_submits.client_order_id(&venue_order_id),
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
                self.order_contexts.venue_order_id(&client_order_id)
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
                        || self.pending_submits.client_order_id(&venue_order_id)
                            == Some(cached_order.client_order_id())
                        || self
                            .order_contexts
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

        if let Some(context) = context {
            if let Some(requested_instrument_id) = requested_instrument_id {
                anyhow::ensure!(
                    context.identity.instrument_id == requested_instrument_id,
                    "registered order instrument {} does not match requested instrument {requested_instrument_id}",
                    context.identity.instrument_id,
                );
            }

            if let Some(cached_order) = cached_order.as_ref() {
                anyhow::ensure!(
                    context.identity.client_order_id == cached_order.client_order_id()
                        && context.identity.instrument_id == cached_order.instrument_id()
                        && context.identity.order_side == cached_order.order_side()
                        && context.identity.order_type == cached_order.order_type()
                        && context.time_in_force == cached_order.time_in_force(),
                    "registered order context for {venue_order_id} contradicts cached order {}",
                    cached_order.client_order_id(),
                );
            }
        }

        Ok(TargetOrderAuthority {
            client_order_id,
            instrument_id: context
                .map(|value| value.identity.instrument_id)
                .or_else(|| cached_order.as_ref().map(Order::instrument_id)),
            order_side: context
                .map(|value| value.identity.order_side)
                .or_else(|| cached_order.as_ref().map(|order| order.order_side())),
            cached_order,
        })
    }

    pub(super) fn query_account_command(&self, _cmd: QueryAccount) {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;
        let collateral_locks = self.collateral_locks.clone();
        let settlement = self.settlement.clone();

        self.spawn_task("query_account", async move {
            fetch_and_emit_account_state(
                &http_client,
                &emitter,
                clock,
                signature_type,
                &collateral_locks,
                &settlement,
            )
            .await
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
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let settlement = self.settlement.clone();
        let order_contexts = self.order_contexts.clone();
        let pending_submits = self.pending_submits.clone();
        let clock = self.clock;
        let signer_type = self.config.signer_type;
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let api_key = SecretString::from(self.secrets.credential.api_key_str().to_string());
        let load_ids = self.config.reconciliation_load_ids().map(Vec::from);
        let cached_filled = cached_order.filled_qty();
        let (filled_before_leg, _) =
            match venue_leg_filled_before_and_quantity(&cached_order, venue_order_id, size_prec) {
                Ok(quantities) => quantities,
                Err(e) => {
                    log::warn!("Cannot query client-bound order {client_order_id}: {e}");
                    return;
                }
            };

        self.spawn_task("query_order", async move {
            let recovering_submit = cached_order.ts_accepted().is_none()
                && pending_submits.client_order_id(&venue_order_id) == Some(client_order_id);

            if !recovering_submit {
                settlement.ensure_order_resolved(&venue_order_id, "QueryOrder")?;
            }

            let order = match http_client.get_order_optional(&venue_order_id_str).await {
                Ok(Some(order)) => order,
                Ok(None) => {
                    log::warn!("Order {venue_order_id_str} not found (empty response)");
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Failed to query order {venue_order_id_str}: {e}");
                    return Ok(());
                }
            };

            let ctx = FillContext {
                signer_type,
                account_id,
                user_address: &user_address,
                api_key: api_key.expose_secret(),
                pusd: get_pusd_currency(),
                clock,
                settlement: settlement.clone(),
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
                    None,
                ),
                clock.get_time_ns(),
            )?;

            if recovering_submit {
                let trades = http_client
                    .get_trades(GetTradesParams {
                        market: Some(order.market.to_string()),
                        ..Default::default()
                    })
                    .await?;

                order_contexts.recover_context(venue_order_id, OrderContext::from(&cached_order));
                if order_contexts.is_closed(venue_order_id) {
                    settlement.note_order_uncertain(
                        venue_order_id,
                        instrument_id,
                        clock.get_time_ns(),
                    );
                    settlement.note_uncertain_submit_closed(&venue_order_id);
                    return Ok(());
                }

                let dispatch = WsDispatchContext {
                    signer_type,
                    token_instruments: &token_instruments,
                    fill_tracker: &fill_tracker,
                    settlement: &settlement,
                    pending_submits: &pending_submits,
                    order_contexts: &order_contexts,
                    emitter: &emitter,
                    account_id,
                    clock,
                    user_address: &user_address,
                    user_api_key: api_key.expose_secret(),
                };

                apply_uncertain_order_evidence(
                    venue_order_id,
                    &order,
                    &trades,
                    &dispatch,
                    &mut ws_dispatch_state.lock(),
                );
                return Ok(());
            }

            let confirmed_filled = if report.filled_qty > cached_filled {
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
                .map(|fills| {
                    ctx.settlement
                        .report_filled_quantities(fills, [&cached_order])
                })
                .transpose()?
                .and_then(|quantities| {
                    quantities
                        .get(&venue_order_id)
                        .copied()
                        .map(|filled| filled_before_leg.as_decimal() + filled)
                })
            } else {
                None
            };

            settlement.ensure_order_resolved(&venue_order_id, "QueryOrder")?;
            cap_order_report_filled_qty(
                &mut report,
                cached_filled,
                confirmed_filled,
                cached_order.non_reopened_voided_qty(),
            );

            if report.order_status == OrderStatus::Canceled
                && ws_dispatch_state
                    .lock()
                    .suppress_modify_cancel_reemit(venue_order_id)
            {
                return Ok(());
            }

            emitter.send_order_status_report(report);
            Ok(())
        });
    }

    pub(super) async fn generate_order_status_report_impl(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let request = self.prepare_order_status_report(cmd)?;
        let collected = self
            .report_client()
            .collect_order_status_report(request)
            .await?;
        self.report_state().finish_order_status_report(collected)
    }

    fn prepare_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<OrderReportRequest> {
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

        if authority.client_order_id.is_some() {
            authority.require_cached_base_limit(venue_order_id)?;
        }

        let instrument = self
            .core
            .cache()
            .instrument(&instrument_id)
            .cloned()
            .with_context(|| format!("instrument {instrument_id} not cached"))?;
        let size_prec = instrument.size_precision();

        Ok(OrderReportRequest {
            authority,
            venue_order_id,
            instrument_id,
            size_prec,
        })
    }

    fn resolve_venue_order_id(
        &self,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> Option<VenueOrderId> {
        venue_order_id
            .or_else(|| client_order_id.and_then(|id| self.order_contexts.venue_order_id(&id)))
            .or_else(|| {
                client_order_id.and_then(|id| {
                    self.core
                        .cache()
                        .order(&id)
                        .and_then(|order| order.venue_order_id())
                })
            })
            .or_else(|| client_order_id.and_then(|id| self.pending_submits.venue_order_id(id)))
    }

    pub(super) async fn generate_order_status_reports_impl(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let pending_promotions = self.ws_dispatch_state.lock().pending_modify_promotions();
        let mut reports = self
            .report_client()
            .collect_orders(cmd, &pending_promotions)
            .await?;
        let ctx = self.fill_context();
        let collection_load_ids = if cmd.instrument_id.is_some() {
            None
        } else {
            self.config.reconciliation_load_ids()
        };

        let mut modify_fill_offsets = AHashMap::new();
        let mut rejected_replacements = AHashSet::new();

        for report in &mut reports {
            let promotion = self
                .ws_dispatch_state
                .lock()
                .pending_modify_promotion(report.venue_order_id);
            let Some(promotion) = promotion else {
                continue;
            };

            let context = self
                .order_contexts
                .get(&promotion.old_venue_order_id)
                .context("pending modification has no old-leg context")?;
            anyhow::ensure!(
                report.quantity == promotion.leg_quantity,
                "replacement venue-leg quantity {} does not match signed quantity {}",
                report.quantity,
                promotion.leg_quantity,
            );
            anyhow::ensure!(
                report.price == Some(promotion.price),
                "replacement venue-leg price {:?} does not match signed price {}",
                report.price,
                promotion.price,
            );
            anyhow::ensure!(
                report.order_side == Some(context.identity.order_side),
                "replacement venue-leg side {:?} does not match logical order side {}",
                report.order_side,
                context.identity.order_side,
            );
            let cached_order = self
                .core
                .cache()
                .order_owned(&promotion.client_order_id)
                .context("pending modification has no cached logical order")?;

            if report.order_status == OrderStatus::Rejected {
                let finish = self
                    .ws_dispatch_state
                    .lock()
                    .finish_modify_without_replacement(
                        promotion.client_order_id,
                        promotion.old_venue_order_id,
                        true,
                        report.ts_last,
                    );

                if let Some((old_venue_order_id, cancel_ts)) = finish {
                    self.emitter.emit_order_modify_rejected(
                        &cached_order,
                        Some(old_venue_order_id),
                        "Replacement order rejected during reconciliation",
                        report.ts_last,
                        None,
                    );

                    if let Some(cancel_ts) = cancel_ts {
                        self.emitter.emit_order_canceled(
                            &cached_order,
                            Some(old_venue_order_id),
                            cancel_ts,
                        );
                    }

                    rejected_replacements.insert(report.venue_order_id);
                    continue;
                }

                if self
                    .order_contexts
                    .venue_order_id(&promotion.client_order_id)
                    != Some(promotion.venue_order_id)
                {
                    continue;
                }
            }

            let promoted = confirm_modify_replacement(
                &cached_order,
                promotion.venue_order_id,
                &self.emitter,
                self.clock,
                &self.fill_tracker,
                &self.settlement,
                &self.order_contexts,
                &self.ws_dispatch_state,
            );

            if !promoted
                && self
                    .order_contexts
                    .venue_order_id(&promotion.client_order_id)
                    != Some(promotion.venue_order_id)
            {
                continue;
            }

            let filled_before_leg = promotion.prior_filled;
            report.client_order_id = Some(promotion.client_order_id);
            report.quantity = promotion.quantity;
            report.filled_qty = filled_before_leg
                .checked_add(report.filled_qty)
                .context("reconciled logical filled quantity overflow")?;
            modify_fill_offsets.insert(report.venue_order_id, filled_before_leg);
        }

        {
            let state = self.ws_dispatch_state.lock();
            reports.retain(|report| {
                !rejected_replacements.contains(&report.venue_order_id)
                    && (report.order_status != OrderStatus::Canceled
                        || !state.suppress_modify_cancel_reemit(report.venue_order_id))
            });
        }

        let logical_fill_offsets = self
            .report_state()
            .logical_fill_offsets(&mut reports, &modify_fill_offsets)?;

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
                    let trades = without_order_status_pending_trades(&ctx.settlement, trades);
                    let (fills, _) = build_fill_reports_from_trades(
                        &trades,
                        &ctx,
                        &self.shared_token_instruments,
                        FillReportScope::new(cmd.instrument_id, None),
                        self.clock.get_time_ns(),
                        collection_load_ids,
                        None,
                    )?;
                    let cache = self.core.cache();
                    let orders = cache.orders_refs(
                        Some(&self.core.venue),
                        cmd.instrument_id.as_ref(),
                        None,
                        Some(&self.core.account_id),
                        None,
                    );
                    ctx.settlement.order_status_filled_quantities(
                        &fills,
                        orders.iter().map(|order| &**order),
                    )?
                }
                Err(e) => {
                    log::warn!("Failed to fetch confirmed fills for open-order check: {e}");
                    Default::default()
                }
            }
        } else {
            Default::default()
        };

        Ok(self.report_state().finish_order_status_reports(
            reports,
            &confirmed_fills,
            &logical_fill_offsets,
            &modify_fill_offsets,
            cmd,
        ))
    }

    pub(super) async fn generate_fill_reports_impl(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let scope = self.prepare_fill_report_scope(&cmd)?;
        let reports = self
            .report_client()
            .collect_fill_reports(&cmd, scope)
            .await?;
        self.settlement.accept_fill_reports(&reports)?;
        Ok(reports.reports)
    }

    fn prepare_fill_report_scope(
        &self,
        cmd: &GenerateFillReports,
    ) -> anyhow::Result<FillReportScope> {
        let authority = cmd
            .venue_order_id
            .map(|venue_order_id| {
                self.resolve_target_order_authority(None, venue_order_id, cmd.instrument_id)
            })
            .transpose()?;

        let instrument_id = cmd
            .instrument_id
            .or_else(|| authority.as_ref().and_then(|value| value.instrument_id));
        Ok(FillReportScope::new(instrument_id, cmd.venue_order_id)
            .with_expected_order_side(authority.as_ref().and_then(|value| value.order_side)))
    }

    pub(super) async fn generate_position_status_reports_impl(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let collected = self
            .report_client()
            .collect_position_status_reports(cmd, self.resolved_balance_scope())
            .await?;
        self.report_state()
            .finish_position_status_reports(collected)
    }

    pub(super) async fn generate_mass_status_impl(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ctx = self.fill_context();

        let (cached_orders, retained_trade_ids) = {
            let cache = self.core.cache();

            let cached_orders = super::reconciliation::cached_order_scopes(&cache, self.core.venue);
            let mut retained_trade_ids: AHashMap<InstrumentId, AHashSet<TradeId>> = AHashMap::new();

            for position in cache.positions_open(
                Some(&self.core.venue),
                None,
                None,
                Some(&self.core.account_id),
                None,
            ) {
                retained_trade_ids
                    .entry(position.instrument_id)
                    .or_default()
                    .extend(position.trade_ids.iter().copied());
            }

            (cached_orders, retained_trade_ids)
        };

        super::reconciliation::generate_mass_status(
            &self.http_client,
            &self.data_api_client,
            &self.shared_token_instruments,
            &ctx,
            &self.core,
            lookback_mins,
            self.config.reconciliation_load_ids(),
            &self.resolved_balance_scope(),
            &retained_trade_ids,
            &cached_orders,
        )
        .await
    }

    fn report_client(&self) -> PolymarketReportClient {
        PolymarketReportClient {
            clock: self.clock,
            account_id: self.core.account_id,
            http_client: self.http_client.clone(),
            data_api_client: self.data_api_client.clone(),
            token_instruments: self.shared_token_instruments.clone(),
            signer_type: self.config.signer_type,
            user_address: self
                .secrets
                .funder
                .clone()
                .unwrap_or_else(|| self.secrets.address.clone()),
            api_key: SecretString::from(self.secrets.credential.api_key_str().to_string()),
            load_ids: self.config.reconciliation_load_ids().map(Vec::from),
            settlement: self.settlement.clone(),
        }
    }

    fn report_state(&self) -> ReportState {
        ReportState {
            core: self.core.clone(),
            settlement: self.settlement.clone(),
            ws_dispatch_state: self.ws_dispatch_state.clone(),
        }
    }

    fn resolved_balance_scope(&self) -> ResolvedBalanceScope {
        ResolvedBalanceScope::from_cache(&self.core.cache(), self.core.venue, self.core.account_id)
    }
}

struct PolymarketReportClient {
    clock: &'static AtomicTime,
    account_id: AccountId,
    http_client: PolymarketClobHttpClient,
    data_api_client: PolymarketDataApiHttpClient,
    token_instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    signer_type: PolymarketSignerType,
    user_address: String,
    api_key: SecretString,
    load_ids: Option<Vec<InstrumentId>>,
    settlement: Arc<SettlementRegistry>,
}

struct OrderReportRequest {
    authority: TargetOrderAuthority,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    size_prec: u8,
}

struct CollectedOrderReport {
    request: OrderReportRequest,
    report: Option<OrderStatusReport>,
    fills: CollectedFillReports,
    trade_ids: Vec<String>,
    has_pending_target: bool,
    total_filled: Decimal,
    avg_px: Option<Decimal>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

struct CollectedPositionReports {
    reports: Vec<(PositionStatusReport, bool)>,
    excluded: Vec<(InstrumentId, bool)>,
}

impl PolymarketReportClient {
    async fn collect_order_status_report(
        &self,
        request: OrderReportRequest,
    ) -> anyhow::Result<CollectedOrderReport> {
        let order = self
            .http_client
            .get_order_optional(request.venue_order_id.as_str())
            .await
            .context("failed to fetch order")?;
        let ctx = self.fill_context();
        let ts_init = self.clock.get_time_ns();

        let report = order
            .as_ref()
            .map(|order| {
                build_target_order_report(
                    order,
                    &self.token_instruments,
                    &ctx,
                    TargetOrderReportScope::new(
                        request.instrument_id,
                        request.venue_order_id,
                        request.authority.client_order_id,
                        request.authority.cached_order.as_ref(),
                        None,
                    ),
                    ts_init,
                )
            })
            .transpose()?;

        let cached_filled = request
            .authority
            .cached_order
            .as_ref()
            .map_or_else(|| Quantity::zero(request.size_prec), Order::filled_qty);
        let needs_trades = report
            .as_ref()
            .is_none_or(|report| report.filled_qty > cached_filled);

        let trades = if needs_trades {
            match self
                .http_client
                .get_trades(GetTradesParams::default())
                .await
            {
                Ok(trades) => trades,
                Err(e) if report.is_some() => {
                    log::warn!("Failed to fetch confirmed fills for order status report: {e}");
                    Vec::new()
                }
                Err(e) => return Err(e).context("failed to fetch trades for order recovery"),
            }
        } else {
            Vec::new()
        };

        let mut venue_order_ids = request
            .authority
            .cached_order
            .as_ref()
            .map(|order| {
                order
                    .venue_order_ids()
                    .into_iter()
                    .copied()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        venue_order_ids.push(request.venue_order_id);
        let trade_ids = trades
            .iter()
            .filter(|trade| {
                venue_order_ids
                    .iter()
                    .any(|id| trade_touches_venue_order(trade, id))
            })
            .map(|trade| trade.id.clone())
            .collect();

        let report_trades = without_order_status_pending_trades(&self.settlement, trades);
        let (fills, discards) = collect_fill_reports_from_trades(
            &report_trades,
            &ctx,
            &self.token_instruments,
            FillReportScope::new(Some(request.instrument_id), Some(request.venue_order_id))
                .with_expected_order_side(
                    request
                        .authority
                        .order_side
                        .or_else(|| report.as_ref().and_then(|report| report.order_side)),
                ),
            ts_init,
            self.load_ids.as_deref(),
            None,
        )?;
        let total_filled = sum_filled_quantity(&fills.reports);
        let avg_px = weighted_average_price(&fills.reports, total_filled);
        let ts_event = fills
            .reports
            .iter()
            .map(|fill| fill.ts_event)
            .max()
            .unwrap_or(ts_init);
        Ok(CollectedOrderReport {
            request,
            report,
            fills,
            trade_ids,
            has_pending_target: discards.has_pending_target,
            total_filled,
            avg_px,
            ts_event,
            ts_init,
        })
    }

    async fn collect_orders(
        &self,
        cmd: &GenerateOrderStatusReports,
        pending_promotions: &[ModifyPromotion],
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let params = crate::http::query::GetOrdersParams::default();
        let mut orders = self
            .http_client
            .get_orders(params)
            .await
            .context("failed to fetch orders")?;

        for promotion in pending_promotions {
            if orders
                .iter()
                .any(|order| order.id == promotion.venue_order_id.as_str())
            {
                continue;
            }

            match self
                .http_client
                .get_order_optional(promotion.venue_order_id.as_str())
                .await
            {
                Ok(Some(order)) => orders.push(order),
                Ok(None) => {}
                Err(e) => {
                    log::warn!(
                        "Failed to reconcile pending replacement {}: {e}",
                        promotion.venue_order_id
                    );
                }
            }
        }

        let ctx = self.fill_context();

        let collection_load_ids = if cmd.instrument_id.is_some() {
            None
        } else {
            self.load_ids.as_deref()
        };

        filter_unsupported_orders(
            &self.http_client,
            &mut orders,
            &self.token_instruments,
            &ctx,
            cmd.instrument_id,
            collection_load_ids,
        )
        .await?;
        let (reports, _) = super::reconciliation::build_order_reports_from_orders(
            &orders,
            &self.token_instruments,
            &ctx,
            cmd.instrument_id,
            self.clock.get_time_ns(),
            collection_load_ids,
        )?;

        Ok(reports)
    }

    async fn collect_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
        cached_filled: AHashMap<VenueOrderId, Option<Decimal>>,
    ) -> anyhow::Result<(Vec<OrderStatusReport>, CollectedFillReports)> {
        let reports = self.collect_orders(cmd, &[]).await?;
        let ctx = self.fill_context();

        let load_ids = if cmd.instrument_id.is_some() {
            None
        } else {
            self.load_ids.as_deref()
        };

        let needs_fills =
            reports
                .iter()
                .any(|report| match cached_filled.get(&report.venue_order_id) {
                    Some(None) => true,
                    filled => {
                        report.filled_qty.as_decimal()
                            > filled.copied().flatten().unwrap_or_default()
                    }
                });

        let fills = if needs_fills {
            match self
                .http_client
                .get_trades(GetTradesParams::default())
                .await
            {
                Ok(trades) => {
                    let trades = without_order_status_pending_trades(&self.settlement, trades);
                    collect_fill_reports_from_trades(
                        &trades,
                        &ctx,
                        &self.token_instruments,
                        FillReportScope::new(cmd.instrument_id, None),
                        self.clock.get_time_ns(),
                        load_ids,
                        None,
                    )?
                    .0
                }
                Err(e) => {
                    log::warn!("Failed to fetch confirmed fills for open-order check: {e}");
                    CollectedFillReports::default()
                }
            }
        } else {
            CollectedFillReports::default()
        };

        Ok((reports, fills))
    }

    async fn collect_fill_reports(
        &self,
        cmd: &GenerateFillReports,
        scope: FillReportScope,
    ) -> anyhow::Result<CollectedFillReports> {
        let trades = self
            .http_client
            .get_trades(super::reconciliation::trades_params_for_window(
                cmd.start, cmd.end,
            ))
            .await
            .context("failed to fetch trades")?;

        let load_ids = if cmd.instrument_id.is_some() || cmd.venue_order_id.is_some() {
            None
        } else {
            self.load_ids.as_deref()
        };

        let (mut reports, _) = collect_fill_reports_from_trades(
            &trades,
            &self.fill_context(),
            &self.token_instruments,
            scope,
            self.clock.get_time_ns(),
            load_ids,
            None,
        )?;
        reports.reports = apply_fill_time_filters(reports.reports, cmd.start, cmd.end);
        log::debug!("Generated {} fill reports", reports.reports.len());
        Ok(reports)
    }

    async fn collect_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
        scope: ResolvedBalanceScope,
    ) -> anyhow::Result<CollectedPositionReports> {
        anyhow::ensure!(
            self.signer_type != PolymarketSignerType::Session,
            "Session positions cannot be inferred from wallet-wide holdings"
        );
        let mut positions = self
            .data_api_client
            .get_positions(&self.user_address)
            .await
            .context("failed to fetch positions from Data API")?;
        let ts_now = self.clock.get_time_ns();
        filter_unsupported_positions(
            &self.http_client,
            &mut positions,
            &self.token_instruments,
            cmd.instrument_id,
            self.load_ids.as_deref(),
            &scope,
        )
        .await?;
        let reports = build_reconciliation_position_reports(
            &positions,
            self.account_id,
            ts_now,
            &self.token_instruments,
            cmd.instrument_id,
            self.load_ids.as_deref(),
            &scope,
        )?;

        let balances: AHashMap<_, _> = positions
            .iter()
            .filter(|position| {
                position_in_report_scope(position, cmd.instrument_id, self.load_ids.as_deref())
            })
            .map(|position| {
                (
                    instrument_id_from_market_token(&position.condition_id, &position.asset),
                    position.redeemable,
                )
            })
            .collect();

        let excluded = balances
            .iter()
            .filter(|(id, redeemable)| scope.excludes_balance(**id, **redeemable))
            .map(|(id, redeemable)| (*id, *redeemable))
            .collect();

        let reports = reports
            .into_iter()
            .map(|report| {
                let redeemable = balances[&report.instrument_id];
                (report, redeemable)
            })
            .collect();

        Ok(CollectedPositionReports { reports, excluded })
    }

    fn fill_context(&self) -> FillContext<'_> {
        FillContext {
            clock: self.clock,
            account_id: self.account_id,
            signer_type: self.signer_type,
            user_address: &self.user_address,
            api_key: self.api_key.expose_secret(),
            pusd: get_pusd_currency(),
            settlement: self.settlement.clone(),
        }
    }
}

struct ReportState {
    core: ExecutionClientCore,
    settlement: Arc<SettlementRegistry>,
    ws_dispatch_state: Arc<Mutex<WsDispatchState>>,
}

impl ReportState {
    fn finish_order_status_report(
        &self,
        mut collected: CollectedOrderReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let report = if let Some(mut report) = collected.report.take() {
            let request = &collected.request;
            let cached = request
                .authority
                .client_order_id
                .and_then(|id| self.core.cache().order_owned(&id))
                .or_else(|| request.authority.cached_order.clone());
            let cached_filled = cached
                .as_ref()
                .map_or_else(|| Quantity::zero(request.size_prec), Order::filled_qty);

            let filled_before = cached
                .as_ref()
                .map(|order| {
                    venue_leg_filled_before_and_quantity(
                        order,
                        request.venue_order_id,
                        request.size_prec,
                    )
                    .map(|(before, _)| before)
                })
                .transpose()?
                .unwrap_or_else(|| Quantity::zero(request.size_prec));

            let quantities = if collected.fills.reports.is_empty() {
                AHashMap::new()
            } else {
                self.settlement
                    .order_status_filled_quantities(&collected.fills.reports, cached.iter())?
            };

            let confirmed_filled = quantities
                .get(&request.venue_order_id)
                .map(|filled| filled_before.as_decimal() + filled);
            cap_order_status_report(
                &mut report,
                cached.as_ref(),
                &self.settlement,
                cached_filled,
                confirmed_filled,
                cached.as_ref().map_or_else(
                    || Quantity::zero(request.size_prec),
                    Order::non_reopened_voided_qty,
                ),
            );
            Some(report)
        } else {
            self.recover_terminal_status(&collected)?
        };

        self.settlement.accept_fill_reports(&collected.fills)?;
        Ok(report.filter(|report| {
            report.order_status != OrderStatus::Canceled
                || !self
                    .ws_dispatch_state
                    .lock()
                    .suppress_modify_cancel_reemit(report.venue_order_id)
        }))
    }

    fn recover_terminal_status(
        &self,
        collected: &CollectedOrderReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let request = &collected.request;
        let authority = &request.authority;
        let venue_order_id = request.venue_order_id;
        let instrument_id = request.instrument_id;
        let size_prec = self
            .core
            .cache()
            .instrument(&instrument_id)
            .with_context(|| format!("instrument {instrument_id} not cached"))?
            .size_precision();
        let ts_init = collected.ts_init;
        let order_fills = &collected.fills.reports;
        let resolved_client_order_id = authority.client_order_id;
        let cached = resolved_client_order_id
            .and_then(|id| self.core.cache().order_owned(&id))
            .or_else(|| authority.cached_order.clone());
        let cached_price = cached.as_ref().and_then(Order::price);

        let awaits_application = order_status_scope_awaits_application(
            &self.settlement,
            &[],
            cached.as_ref(),
            &venue_order_id,
        ) || collected
            .trade_ids
            .iter()
            .any(|id| self.settlement.omits_order_status_quantity(id));
        let keeps_cached_fill = cached
            .as_ref()
            .is_some_and(|order| self.settlement.order_status_keeps_cached_fill(order));

        if collected.has_pending_target || awaits_application || keeps_cached_fill {
            let Some(cached) = cached.as_ref() else {
                log::debug!(
                    "Order {venue_order_id} has unsettled trades but no cached order; deferring recovery"
                );
                return Ok(None);
            };

            let order_status = order_status_recovery_status(
                cached.status(),
                cached.filled_qty().is_zero(),
                keeps_cached_fill,
            );

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

        if order_fills.is_empty() {
            let Some(cached) = cached.as_ref() else {
                log::debug!(
                    "Order {venue_order_id} not active at venue, no trades found, and no cached order; nothing to recover"
                );
                return Ok(None);
            };

            if cached.ts_accepted().is_none() {
                return Ok(None);
            }

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

        let Some(cached_order) = cached.as_ref() else {
            log::debug!(
                "Order {venue_order_id} has trades but no cached order; deferring to engine"
            );
            return Ok(None);
        };

        let quantity = cached_order.quantity();
        let (filled_before, leg_quantity) =
            venue_leg_filled_before_and_quantity(cached_order, venue_order_id, size_prec)?;
        let avg_px = collected.avg_px;
        let raw_filled_qty = Quantity::from_decimal_dp(collected.total_filled, size_prec)?;
        anyhow::ensure!(
            raw_filled_qty.as_decimal() == collected.total_filled,
            "recovered fills cannot be represented exactly"
        );
        let ts_event = collected.ts_event;

        let order_status = recovered_terminal_order_status(
            cached_order.time_in_force(),
            leg_quantity,
            raw_filled_qty,
        );
        let filled_qty = filled_before
            .checked_add(raw_filled_qty)
            .context("logical filled quantity overflow")?;

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
            cached_order.order_side().into(),
            cached_order.order_type(),
            cached_order.time_in_force(),
            order_status,
            quantity,
            filled_qty,
            ts_event,
            ts_event,
            ts_init,
            None,
        );
        report.price = cached_price;

        report.avg_px = if filled_before.is_zero() {
            avg_px
        } else {
            None
        };

        normalize_terminal_order_report_quantity(
            &mut report,
            cached_order.non_reopened_voided_qty(),
        );

        Ok(Some(report))
    }

    fn logical_fill_offsets(
        &self,
        reports: &mut [OrderStatusReport],
        modify_fill_offsets: &AHashMap<VenueOrderId, Quantity>,
    ) -> anyhow::Result<AHashMap<VenueOrderId, Quantity>> {
        let mut logical_fill_offsets = modify_fill_offsets.clone();
        for report in reports {
            if logical_fill_offsets.contains_key(&report.venue_order_id) {
                continue;
            }

            let cached_order = report
                .client_order_id
                .and_then(|id| self.core.cache().order_owned(&id))
                .or_else(|| {
                    self.core
                        .cache()
                        .client_order_id(&report.venue_order_id)
                        .and_then(|id| self.core.cache().order_owned(id))
                });

            if let Some(cached_order) = cached_order {
                let filled_before = apply_logical_order_quantity(report, &cached_order)?;
                logical_fill_offsets.insert(report.venue_order_id, filled_before);
            }
        }

        Ok(logical_fill_offsets)
    }

    fn finish_order_status_reports(
        &self,
        mut reports: Vec<OrderStatusReport>,
        confirmed_fills: &AHashMap<VenueOrderId, Decimal>,
        logical_fill_offsets: &AHashMap<VenueOrderId, Quantity>,
        modify_fill_offsets: &AHashMap<VenueOrderId, Quantity>,
        cmd: &GenerateOrderStatusReports,
    ) -> Vec<OrderStatusReport> {
        for report in &mut reports {
            let cached_order = report
                .client_order_id
                .and_then(|id| self.core.cache().order_owned(&id));
            let filled_before_leg = logical_fill_offsets
                .get(&report.venue_order_id)
                .copied()
                .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
            let cached_filled = cached_order.as_ref().map_or_else(
                || Quantity::zero(report.quantity.precision),
                Order::filled_qty,
            );

            let non_reopened_voided = cached_order.as_ref().map_or_else(
                || Quantity::zero(report.quantity.precision),
                Order::non_reopened_voided_qty,
            );

            cap_order_status_report(
                report,
                cached_order.as_ref(),
                &self.settlement,
                cached_filled,
                confirmed_fills
                    .get(&report.venue_order_id)
                    .copied()
                    .map(|filled| filled_before_leg.as_decimal() + filled),
                non_reopened_voided,
            );
        }

        reports.retain(|report| {
            !cmd.open_only
                || report.order_status.is_open()
                || modify_fill_offsets.contains_key(&report.venue_order_id)
        });

        log::debug!("Generated {} order status reports", reports.len());
        reports
    }

    fn finish_position_status_reports(
        &self,
        collected: CollectedPositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let scope = ResolvedBalanceScope::from_cache(
            &self.core.cache(),
            self.core.venue,
            self.core.account_id,
        );
        anyhow::ensure!(
            collected
                .excluded
                .iter()
                .all(|(id, redeemable)| scope.excludes_balance(*id, *redeemable)),
            "Polymarket balance became reportable during collection; retry position reports"
        );

        let reports: Vec<_> = collected
            .reports
            .into_iter()
            .filter(|(report, redeemable)| {
                !scope.excludes_balance(report.instrument_id, *redeemable)
            })
            .map(|(report, _)| report)
            .collect();

        log::debug!("Generated {} position status reports", reports.len());
        Ok(reports)
    }
}

fn fill_report_gate(
    settlement: &SettlementRegistry,
    cmd: &GenerateFillReports,
) -> anyhow::Result<()> {
    match cmd.venue_order_id {
        Some(id) => settlement.ensure_order_resolved(&id, "fill reports"),
        None => settlement.ensure_resolved(cmd.instrument_id, "fill reports"),
    }
}

fn cap_order_status_report(
    report: &mut OrderStatusReport,
    cached_order: Option<&OrderAny>,
    settlement: &SettlementRegistry,
    cached_filled: Quantity,
    confirmed_filled: Option<Decimal>,
    non_reopened_voided: Quantity,
) {
    // Another confirmed trade must not raise this report while a fill or void on the order is
    // still waiting. Core would infer that difference, then apply the real fill again.
    if let Some(order) = cached_order.filter(|order| {
        settlement.order_status_keeps_cached_fill(order)
            || order_status_scope_awaits_application(
                settlement,
                &[],
                Some(order),
                &report.venue_order_id,
            )
    }) {
        report.filled_qty = order.filled_qty();
        report.order_status = order.status();
        return;
    }

    cap_order_report_filled_qty(report, cached_filled, confirmed_filled, non_reopened_voided);
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
    signature_type: PolymarketSignatureType,
    collateral_locks: &Mutex<CollateralLocks>,
    settlement: &SettlementRegistry,
) -> anyhow::Result<()> {
    let mut ticket = collateral_locks.lock().begin_refresh(settlement);

    let params = GetBalanceAllowanceParams {
        asset_type: Some(crate::http::query::AssetType::Collateral),
        signature_type: Some(signature_type),
        ..Default::default()
    };

    if !ticket.confirmed.is_empty() {
        http_client
            .update_balance_allowance(params.clone())
            .await
            .context("failed to refresh confirmed Polymarket collateral")?;
    }

    let balance = http_client
        .get_balance(params)
        .await
        .context("failed to fetch balance")?;

    let pusd = get_pusd_currency();
    let account_balance =
        parse_balance_allowance(balance, pusd).context("failed to parse balance")?;
    let now = clock.get_time_ns();

    let mut locks = collateral_locks.lock();
    let confirmed = settlement.confirmed_fills();
    ticket
        .confirmed
        .retain(|trade_id| confirmed.contains(trade_id));

    let AppliedBalance::Fresh(account_balance) = locks
        .apply_refresh(ticket, account_balance.total, now)
        .context("failed to apply Polymarket balance reservations")?
    else {
        log::debug!("Skipping stale Polymarket balance refresh");
        return Ok(());
    };

    emitter.emit_account_state(vec![account_balance], vec![], true, now, None);
    drop(confirmed);
    drop(locks);

    log::debug!(
        "Account state updated: balance={} pUSD",
        account_balance.total
    );
    Ok(())
}

pub(super) async fn fetch_collateral_balance_pusd(
    http_client: &PolymarketClobHttpClient,
    signature_type: PolymarketSignatureType,
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
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::{Json, Router, routing::get};
    use nautilus_common::{
        cache::Cache,
        clients::ExecutionClient,
        messages::{
            ExecutionEvent,
            execution::{
                GenerateFillReportsBuilder, GenerateOrderStatusReportBuilder,
                GenerateOrderStatusReportsBuilder, GeneratePositionStatusReportsBuilder,
            },
        },
    };
    use nautilus_core::Params;
    use nautilus_model::{
        enums::{AccountType, OmsType, TimeInForce},
        identifiers::{AccountId, ClientId, Symbol, TraderId},
        instruments::stubs::binary_option,
        types::{AccountBalance, Money},
    };
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::{
        common::{consts::MARKET_VERSION_KEY, credential::Credential},
        config::PolymarketExecutionClientConfig,
    };

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn fill_report_receipt_defers_collateral_marks_until_acceptance(#[case] accept: bool) {
        let trade: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json")).unwrap();
        let (mut client, server) = report_test_client(vec![trade.clone()]).await;
        let mut binary = binary_option();
        binary.id = InstrumentId::from(
            format!(
                "{}-{}.POLYMARKET",
                trade["market"].as_str().unwrap(),
                trade["asset_id"].as_str().unwrap()
            )
            .as_str(),
        );
        binary.raw_symbol = Symbol::from(trade["asset_id"].as_str().unwrap());
        binary.currency = Currency::pUSD();
        binary.outcome = Some(Ustr::from(trade["outcome"].as_str().unwrap()));
        binary.price_precision = 4;
        binary.size_precision = 4;
        binary.price_increment = "0.0001".parse().unwrap();
        binary.size_increment = Quantity::from("0.0001");
        binary.info = Some(Params::new(
            [
                (MARKET_VERSION_KEY.to_string(), json!("v1")),
                ("condition_id".to_string(), trade["market"].clone()),
            ]
            .into(),
        ));
        let instrument = InstrumentAny::BinaryOption(binary);
        client.on_instrument(instrument);
        let command = GenerateFillReportsBuilder::default()
            .ts_init(UnixNanos::from(1))
            .build()
            .unwrap();
        let task = client.generate_fill_reports_task(&command).unwrap();

        nautilus_common::live::get_runtime()
            .spawn(task.collection)
            .await
            .unwrap();
        let trade_id = TradeId::from(trade["id"].as_str().unwrap());
        assert!(!client.settlement.settled_on_arrival(&trade_id));

        if accept {
            assert_eq!((task.finish)().unwrap().len(), 1);
        } else {
            drop(task.finish);
        }

        assert_eq!(client.settlement.settled_on_arrival(&trade_id), accept);

        let retry = client.generate_fill_reports_task(&command).unwrap();

        nautilus_common::live::get_runtime()
            .spawn(retry.collection)
            .await
            .unwrap();
        let reports = (retry.finish)().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].trade_id, trade_id);
        assert!(client.settlement.settled_on_arrival(&trade_id));
        server.abort();
    }

    #[rstest]
    #[case("order status report")]
    #[case("order status reports")]
    #[case("fill reports")]
    #[case("position status reports")]
    #[tokio::test]
    async fn report_tasks_recheck_settlement_before_acceptance(#[case] report: &str) {
        let (client, server) = report_test_client(Vec::new()).await;
        let task = empty_report_task(&client, report);

        nautilus_common::live::get_runtime()
            .spawn(task.collection)
            .await
            .unwrap();
        client
            .settlement
            .quarantine_invalid_trade("trade-during-collection");

        let error = (task.finish)().unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "cannot generate {report}: Polymarket settlement registry holds 1 record(s) with unresolved evidence"
            )
        );
        server.abort();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn bulk_report_task_preserves_inline_promotion_boundary(
        #[case] pending_before_collection: bool,
    ) {
        let (client, server) = report_test_client(Vec::new()).await;
        let task = (!pending_before_collection)
            .then(|| empty_report_task(&client, "order status reports"));
        let client_order_id = ClientOrderId::from("O-MODIFY");
        let instrument_id = *client.core.cache().instrument_ids(None)[0];
        {
            let mut state = client.ws_dispatch_state.lock();
            assert!(state.begin_modify(client_order_id, VenueOrderId::from("OLD"), instrument_id));
            assert!(state.set_modify_replacement(
                client_order_id,
                VenueOrderId::from("NEW"),
                Quantity::from("12"),
                Quantity::from("10"),
                Quantity::from("2"),
                "0.5000".parse().unwrap()
            ));
        }

        let command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::from(1))
            .open_only(false)
            .build()
            .unwrap();
        assert!(
            client
                .generate_order_status_reports_task(&command)
                .is_none()
        );

        if let Some(task) = task {
            nautilus_common::live::get_runtime()
                .spawn(task.collection)
                .await
                .unwrap();
            assert_eq!(
                (task.finish)().unwrap_err().to_string(),
                "pending Polymarket modification requires inline order reconciliation"
            );
        }

        assert_eq!(
            client
                .ws_dispatch_state
                .lock()
                .pending_modify_promotions()
                .len(),
            1
        );
        server.abort();
    }

    fn empty_report_task(
        client: &PolymarketExecutionClient,
        report: &str,
    ) -> ExecutionReportTask<()> {
        match report {
            "order status report" => {
                let instrument_id = *client.core.cache().instrument_ids(None)[0];
                let command = GenerateOrderStatusReportBuilder::default()
                    .ts_init(UnixNanos::from(1))
                    .instrument_id(Some(instrument_id))
                    .venue_order_id(Some(VenueOrderId::from("V-EMPTY")))
                    .build()
                    .unwrap();
                discard_report_output(client.generate_order_status_report_task(&command).unwrap())
            }
            "order status reports" => {
                let command = GenerateOrderStatusReportsBuilder::default()
                    .ts_init(UnixNanos::from(1))
                    .open_only(false)
                    .build()
                    .unwrap();
                discard_report_output(client.generate_order_status_reports_task(&command).unwrap())
            }
            "fill reports" => {
                let command = GenerateFillReportsBuilder::default()
                    .ts_init(UnixNanos::from(1))
                    .build()
                    .unwrap();
                discard_report_output(client.generate_fill_reports_task(&command).unwrap())
            }
            "position status reports" => {
                let command = GeneratePositionStatusReportsBuilder::default()
                    .ts_init(UnixNanos::from(1))
                    .build()
                    .unwrap();
                discard_report_output(
                    client
                        .generate_position_status_reports_task(&command)
                        .unwrap(),
                )
            }
            _ => unreachable!(),
        }
    }

    fn discard_report_output<T: 'static>(task: ExecutionReportTask<T>) -> ExecutionReportTask<()> {
        ExecutionReportTask {
            collection: task.collection,
            finish: Box::new(move || (task.finish)().map(|_| ())),
        }
    }

    async fn report_test_client(
        trades: Vec<serde_json::Value>,
    ) -> (PolymarketExecutionClient, tokio::task::JoinHandle<()>) {
        let router = Router::new().fallback(move |uri: axum::http::Uri| {
            let trades = trades.clone();
            async move {
                Json(if uri.path().starts_with("/data/order/") {
                    serde_json::Value::Null
                } else if uri.path() == "/data/trades" {
                    json!({"data": trades, "next_cursor": "LTE="})
                } else if uri.path() == "/v2/positions" {
                    let mut page = serde_json::from_str::<serde_json::Value>(include_str!(
                        "../../test_data/data_api_positions_response.json"
                    ))
                    .unwrap();
                    page["data"] = json!([]);
                    page
                } else {
                    serde_json::from_str::<serde_json::Value>(include_str!(
                        "../../test_data/http_empty_page.json"
                    ))
                    .unwrap()
                })
            }
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::BinaryOption(binary_option()))
            .unwrap();

        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("POLYMARKET"),
            *crate::common::consts::POLYMARKET_VENUE,
            OmsType::Netting,
            AccountId::from("POLYMARKET-001"),
            AccountType::Cash,
            None,
            cache,
        );

        let config = PolymarketExecutionClientConfig {
            private_key: Some(
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".into(),
            ),
            api_key: Some("00000000-0000-0000-0000-000000000001".into()),
            api_secret: Some("dGVzdA==".into()),
            passphrase: Some("test-pass".into()),
            base_url_http: Some(format!("http://{addr}")),
            base_url_data_api: Some(format!("http://{addr}")),
            ..Default::default()
        };

        (
            PolymarketExecutionClient::new(core, config).unwrap(),
            server,
        )
    }

    #[tokio::test]
    async fn overlapping_account_refreshes_publish_only_the_newest_balance() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let requests = Arc::new(AtomicUsize::new(0));

        let app = Router::new().route(
            "/balance-allowance",
            get({
                let started = started.clone();
                let release = release.clone();
                let requests = requests.clone();
                move || {
                    let started = started.clone();
                    let release = release.clone();
                    let requests = requests.clone();
                    async move {
                        let first = requests.fetch_add(1, Ordering::AcqRel) == 0;
                        if first {
                            started.notify_one();
                            release.notified().await;
                        }

                        Json(json!({"balance": if first { "1011000000" } else { "1012000000" }}))
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let credential =
            Credential::new("test-key".into(), "dGVzdA==".into(), "test-pass".into()).unwrap();
        let client = PolymarketClobHttpClient::new(
            credential,
            "0x1111111111111111111111111111111111111111".to_string(),
            Some(format!("http://{addr}")),
            5,
        )
        .unwrap();
        let account_id = AccountId::from("POLYMARKET-001");
        let clock: &'static AtomicTime =
            Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(1))));

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Cash,
            Some(Currency::pUSD()),
        );
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let locks = Arc::new(Mutex::new(CollateralLocks::default()));
        let settlement = Arc::new(SettlementRegistry::new(account_id));

        let first = tokio::spawn({
            let client = client.clone();
            let emitter = emitter.clone();
            let locks = locks.clone();
            let settlement = settlement.clone();
            async move {
                fetch_and_emit_account_state(
                    &client,
                    &emitter,
                    clock,
                    PolymarketSignatureType::Eoa,
                    &locks,
                    &settlement,
                )
                .await
                .unwrap();
            }
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            started.notified().await;
            fetch_and_emit_account_state(
                &client,
                &emitter,
                clock,
                PolymarketSignatureType::Eoa,
                &locks,
                &settlement,
            )
            .await
            .unwrap();
            release.notify_one();
            first.await.unwrap();
        })
        .await
        .unwrap();

        server.abort();

        let ExecutionEvent::Account(event) = events.try_recv().unwrap() else {
            panic!("expected account state");
        };

        assert_eq!(requests.load(Ordering::Acquire), 2);
        assert_eq!(event.account_id, account_id);
        assert_eq!(event.account_type, AccountType::Cash);
        assert_eq!(event.base_currency, Some(Currency::pUSD()));
        assert!(event.is_reported);
        assert!(event.margins.is_empty());
        assert_eq!(event.ts_event, UnixNanos::from(1));
        assert_eq!(event.ts_init, UnixNanos::from(1));
        assert_eq!(event.info, None);
        assert_eq!(
            event.balances,
            vec![AccountBalance::new(
                Money::from("1012 pUSD"),
                Money::from("0 pUSD"),
                Money::from("1012 pUSD"),
            )]
        );
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

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

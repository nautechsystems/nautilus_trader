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

use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
};

use anyhow::Context;
use nautilus_live::execution::failure::CommandFailure;
use nautilus_model::{enums::OrderStatus, orders::Order};
use nautilus_network::http::Method;
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;

use super::{
    mutation::{Mutation, RECOVERY_POLL_INTERVAL},
    submit::{self, Submission},
};
use crate::{
    http::{
        client::KalshiHttpClient,
        error::KalshiHttpError,
        models::{
            BatchCancelOrder, BatchCancelResponse, BatchCreateOrder, BatchCreateResponse,
            CreateOrderResponse, Field,
        },
        query::{BatchCancelRequest, BatchCreateRequest, CancelOrderRequest},
    },
    websocket::client::KalshiWebSocketClient,
};

pub(super) struct SubmissionBatch {
    pub(super) submissions: Vec<Submission>,
}

impl SubmissionBatch {
    pub(super) async fn run(mut self) {
        let epoch = self.submissions[0].ws_client.connection_epoch();
        let cancellation = self.submissions[0].cancellation.clone();
        let prepared = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(anyhow::anyhow!("Kalshi list canceled before dispatch")),
            result = self.prepare() => result,
        };

        let (holds, size) = match prepared {
            Ok(value) => value,
            Err(e) => {
                self.deny(&e.to_string());
                return;
            }
        };

        if let Err(e) = self.admit(holds) {
            self.deny(&e.to_string());
            return;
        }

        let mut blocked = false;
        for chunk in self.submissions.chunks(size) {
            if blocked {
                for child in chunk {
                    child.reject(submit::NOT_SENT_REASON, false);
                }

                continue;
            }

            blocked = !submit_orders(chunk, epoch).await;
        }
    }

    async fn prepare(&self) -> anyhow::Result<(Vec<Decimal>, usize)> {
        let size = self.submissions[0].http.batch_size(Method::POST).await?;
        let mut holds = Vec::with_capacity(self.submissions.len());
        for child in &self.submissions {
            holds.push(
                submit::prepare(&child.http, &child.order, &child.definition, &child.request)
                    .await?,
            );
        }

        Ok((holds, size))
    }

    fn admit(&mut self, holds: Vec<Decimal>) -> anyhow::Result<()> {
        let state = Arc::clone(&self.submissions[0].state);
        let mut state = state.lock().expect("private state lock is not poisoned");
        self.submissions
            .iter_mut()
            .zip(holds)
            .try_for_each(|(child, hold)| child.admit(&mut state, hold))
    }

    fn deny(&mut self, reason: &str) {
        for child in &mut self.submissions {
            if child.order.status() == OrderStatus::Initialized {
                child.deny(&format!("VALIDATION_FAILED: {reason}"));
            } else {
                child.reject(submit::NOT_SENT_REASON, false);
            }
        }
    }
}

pub(super) struct CancellationBatch {
    pub(super) mutations: Vec<Mutation>,
    pub(super) epoch: Option<(u64, u64)>,
}

impl CancellationBatch {
    pub(super) async fn run(mut self) {
        if self.mutations.is_empty() {
            return;
        }

        let cancellation = self.mutations[0].cancellation.clone();

        while !self.mutations.is_empty() {
            let prepared = tokio::select! {
                biased;
                () = cancellation.cancelled() => Err(anyhow::anyhow!("Kalshi batch cancel stopped before dispatch")),
                result = self.prepare() => result,
            };

            let size = match prepared {
                Ok(size) => size,
                Err(e) => {
                    for child in &self.mutations {
                        child.reject_validation(&e.to_string());
                    }

                    return;
                }
            };

            let (prepared, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mutations)
                .into_iter()
                .partition(|child| child.order.venue_order_id().is_some());
            self.mutations = pending;

            if !cancel_chunks(&prepared, size, self.epoch).await {
                reject_cancellations(&self.mutations);
                return;
            }
        }
    }

    async fn prepare(&mut self) -> anyhow::Result<usize> {
        let epoch = self.epoch;
        let size = self.mutations[0].http.batch_size(Method::DELETE).await?;

        loop {
            let first = &self.mutations[0];
            let state = Arc::clone(&first.state);

            let (prepared, resolved) = {
                let mut state = state.lock().expect("private state lock is not poisoned");
                anyhow::ensure!(
                    epoch.is_some() && first.ws_client.connection_epoch() == epoch,
                    "Kalshi batch cancellation generation changed"
                );
                let barrier = state.barrier.lock().expect("settlement barrier lock");
                let ready = first.ready.load(Ordering::Acquire)
                    && state.epoch == epoch
                    && barrier.is_observed()
                    && !barrier.is_blocked();
                drop(barrier);
                let mut resolved = Vec::new();

                for child in &mut self.mutations {
                    if let Some(current) = state.orders.get(&child.order.client_order_id()) {
                        anyhow::ensure!(
                            current.order.instrument_id() == child.order.instrument_id()
                                && (current.order.strategy_id() == child.order.strategy_id()
                                    || current.order.strategy_id().is_external())
                                && child
                                    .order
                                    .venue_order_id()
                                    .is_none_or(|id| Some(id) == current.order.venue_order_id()),
                            "Kalshi batch cancellation private identity mismatch"
                        );

                        if current.order.is_closed() {
                            resolved.push(child.order.client_order_id());
                        } else if current.order.venue_order_id().is_some()
                            && !current.order.strategy_id().is_external()
                        {
                            child.order = current.order.clone();
                        }
                    }
                }

                for id in &resolved {
                    state.mutations.remove(id);
                }

                (
                    ready
                        && self
                            .mutations
                            .iter()
                            .any(|child| child.order.venue_order_id().is_some()),
                    resolved,
                )
            };

            self.mutations
                .retain(|child| !resolved.contains(&child.order.client_order_id()));

            if prepared || self.mutations.is_empty() {
                return Ok(size);
            }

            nautilus_network::dst::time::sleep(RECOVERY_POLL_INTERVAL).await;
        }
    }
}

async fn submit_orders(chunk: &[Submission], epoch: Option<(u64, u64)>) -> bool {
    let first = &chunk[0];

    let current = epoch.is_some() && first.ws_client.connection_epoch() == epoch;

    if !current || first.cancellation.is_cancelled() {
        for child in chunk {
            child.reject(submit::NOT_SENT_REASON, false);
        }

        return false;
    }

    first.ready.store(false, Ordering::Release);

    let request = BatchCreateRequest {
        orders: chunk.iter().map(|child| child.request.clone()).collect(),
    };

    let http = transport(&first.http, &first.ws_client, &first.cancellation, epoch);

    let result = http.create_batch(&request, false).await;
    let complete = resolve_submissions(chunk, result);
    let _ = first.refresh.send(());
    complete
}

fn resolve_submissions(
    chunk: &[Submission],
    result: Result<BatchCreateResponse, KalshiHttpError>,
) -> bool {
    let first = &chunk[0];

    let response = match result {
        Ok(response) => response,
        Err(e) => {
            if matches!(submit::classify_failure(&e), CommandFailure::NotSent(_)) {
                for child in chunk {
                    child.reject(&submit::rejection_reason(&e), false);
                }
            } else {
                first.invalidate();
                log::warn!("Kalshi batch submission requires recovery: {e}");
            }

            return false;
        }
    };

    let identities = correlate(&response.orders, |item| match &item.client_order_id {
        Field::Value(id) => Some(id.as_str()),
        _ => None,
    });

    let venue_ids = correlate(&response.orders, |item| match &item.order_id {
        Field::Value(id) => Some(id.as_str()),
        _ => None,
    });

    let mut complete = true;

    for child in chunk {
        let id = child.order.client_order_id().to_string();

        let Some(item) = identities.get(id.as_str()).copied().flatten() else {
            complete = false;
            continue;
        };

        let unique = matches!(&item.order_id, Field::Value(id)
            if venue_ids.get(id.as_str()).copied().flatten().is_some());

        complete &= resolve_submission(child, item, unique);
    }

    if !complete {
        first.invalidate();
        log::warn!("Kalshi batch submission contains unresolved item outcomes");
    }

    complete
}

fn resolve_submission(child: &Submission, item: &BatchCreateOrder, unique: bool) -> bool {
    if let Field::Value(error) = &item.error {
        if matches!(&item.order_id, Field::Value(id) if !id.is_empty())
            || matches!(item.ts_ms, Field::Value(_))
            || matches!(item.fill_count, Field::Value(_))
            || matches!(item.remaining_count, Field::Value(_))
        {
            return false;
        }

        let due_post_only = submit::post_only_error(error);

        let code = if due_post_only {
            "post_only_cross"
        } else {
            match &error.code {
                Field::Value(code) => code.as_str(),
                _ => "",
            }
        };

        let Some(reason) = submit::rejection_code(code) else {
            return false;
        };

        child.reject(reason, due_post_only);
        return true;
    }

    if !unique {
        return false;
    }

    match acknowledgement(item) {
        Ok(ack) => child.resolve(Ok(ack)),
        Err(_) => false,
    }
}

async fn cancel_chunks(mutations: &[Mutation], size: usize, epoch: Option<(u64, u64)>) -> bool {
    for (index, chunk) in mutations.chunks(size).enumerate() {
        if !cancel_orders(chunk, epoch).await {
            reject_cancellations(&mutations[((index + 1) * size).min(mutations.len())..]);
            return false;
        }
    }

    true
}

fn reject_cancellations(mutations: &[Mutation]) {
    for child in mutations {
        child.reject_validation(submit::NOT_SENT_REASON);
    }
}

async fn cancel_orders(chunk: &[Mutation], epoch: Option<(u64, u64)>) -> bool {
    let first = &chunk[0];
    let admitted = tokio::select! {
        biased;
        () = first.cancellation.cancelled() => Err(anyhow::anyhow!("Kalshi batch cancel stopped before dispatch")),
        result = prepare_cancellations(chunk, epoch) => result,
    };

    let chunk = match admitted {
        Ok(chunk) => chunk,
        Err(e) => {
            for child in chunk {
                child.reject_validation(&e.to_string());
            }

            return false;
        }
    };

    if chunk.is_empty() {
        return true;
    }

    let request = BatchCancelRequest {
        orders: chunk
            .iter()
            .map(|child| CancelOrderRequest {
                order_id: child
                    .order
                    .venue_order_id()
                    .expect("prepared cancellation identity")
                    .to_string(),
                subaccount: child.subaccount,
                exchange_index: child.shard,
                market_ticker: Some(child.order.instrument_id().symbol.to_string()),
            })
            .collect(),
    };

    let http = transport(&first.http, &first.ws_client, &first.cancellation, epoch);

    let result = http.cancel_batch(&request, false).await;
    let complete = resolve_cancellations(&chunk, result);
    let _ = first.refresh.send(());
    complete
}

async fn prepare_cancellations(
    chunk: &[Mutation],
    epoch: Option<(u64, u64)>,
) -> anyhow::Result<Vec<&Mutation>> {
    loop {
        if let Some(prepared) = admit_cancellations(chunk, epoch)? {
            return Ok(prepared);
        }

        nautilus_network::dst::time::sleep(RECOVERY_POLL_INTERVAL).await;
    }
}

fn admit_cancellations(
    chunk: &[Mutation],
    epoch: Option<(u64, u64)>,
) -> anyhow::Result<Option<Vec<&Mutation>>> {
    let first = &chunk[0];
    let mut state = first
        .state
        .lock()
        .expect("private state lock is not poisoned");
    anyhow::ensure!(
        !first.cancellation.is_cancelled()
            && epoch.is_some()
            && first.ws_client.connection_epoch() == epoch,
        "Kalshi batch cancel generation changed"
    );
    let barrier = state.barrier.lock().expect("settlement barrier lock");
    let ready = first.ready.load(Ordering::Acquire)
        && state.epoch == epoch
        && barrier.is_observed()
        && !barrier.is_blocked();
    drop(barrier);

    if !ready {
        return Ok(None);
    }

    let mut prepared = Vec::with_capacity(chunk.len());

    for child in chunk {
        let order = &state
            .orders
            .get(&child.order.client_order_id())
            .context("Kalshi cancellation context missing")?
            .order;
        anyhow::ensure!(
            order.venue_order_id() == child.order.venue_order_id(),
            "Kalshi batch cancellation identity or state changed"
        );

        if order.is_closed() {
            state.mutations.remove(&child.order.client_order_id());
        } else {
            prepared.push(child);
        }
    }

    for child in &prepared {
        child.mark_dispatched(&mut state, None)?;
    }

    Ok(Some(prepared))
}

fn resolve_cancellations(
    chunk: &[&Mutation],
    result: Result<BatchCancelResponse, KalshiHttpError>,
) -> bool {
    let response = match result {
        Ok(response) => response,
        Err(e) => {
            if matches!(submit::classify_failure(&e), CommandFailure::NotSent(_)) {
                for child in chunk {
                    child.reject_validation(&submit::rejection_reason(&e));
                }
            } else {
                log::warn!("Kalshi batch cancellation requires recovery: {e}");
            }

            return false;
        }
    };

    let identities = correlate(&response.orders, |item| Some(item.order_id.as_str()));
    let mut complete = true;

    for child in chunk {
        let id = child
            .order
            .venue_order_id()
            .expect("prepared cancellation identity");

        let Some(item) = identities.get(id.as_str()).copied().flatten() else {
            complete = false;
            continue;
        };

        complete &= resolve_cancellation(child, item);
    }

    if !complete {
        log::warn!("Kalshi batch cancellation contains unresolved item outcomes");
    }

    complete
}

fn resolve_cancellation(child: &Mutation, item: &BatchCancelOrder) -> bool {
    if matches!(&item.client_order_id, Field::Value(id)
        if id != child.order.client_order_id().as_str())
    {
        return false;
    }

    if let Field::Value(error) = &item.error {
        if matches!(item.ts_ms, Field::Value(_)) || !item.reduced_by.0.is_zero() {
            return false;
        }

        let code = match &error.code {
            Field::Value(code) => code.as_str(),
            _ => "",
        };

        let Some(reason) = submit::rejection_code(code) else {
            return false;
        };

        child.reject(reason);
        return true;
    }

    let Field::Value(ts_ms) = item.ts_ms else {
        return false;
    };

    if item.reduced_by.0 < Decimal::ZERO || item.reduced_by.0.normalize().scale() > 2 {
        return false;
    }

    let client_id = if item.client_order_id == Field::Null {
        &Field::Missing
    } else {
        &item.client_order_id
    };

    if let Err(e) = child.acknowledge(&item.order_id, client_id, ts_ms, None) {
        log::warn!("Kalshi batch cancellation requires recovery: {e}");
        return false;
    }

    true
}

fn transport(
    http: &KalshiHttpClient,
    ws_client: &KalshiWebSocketClient,
    cancellation: &CancellationToken,
    epoch: Option<(u64, u64)>,
) -> KalshiHttpClient {
    let ws_client = ws_client.clone();
    let cancellation = cancellation.clone();
    http.clone()
        .with_dispatch_check(cancellation.clone(), move || {
            !cancellation.is_cancelled() && epoch.is_some() && ws_client.connection_epoch() == epoch
        })
}

fn acknowledgement(result: &BatchCreateOrder) -> anyhow::Result<CreateOrderResponse> {
    let (
        Field::Value(order_id),
        Field::Value(fill_count),
        Field::Value(remaining_count),
        Field::Value(ts_ms),
    ) = (
        &result.order_id,
        result.fill_count.clone(),
        result.remaining_count.clone(),
        result.ts_ms.clone(),
    )
    else {
        anyhow::bail!("incomplete Kalshi batch acknowledgement");
    };

    anyhow::ensure!(!order_id.is_empty(), "empty Kalshi batch venue identity");
    Ok(CreateOrderResponse {
        order_id: order_id.clone(),
        fill_count,
        remaining_count,
        ts_ms,
        client_order_id: result.client_order_id.clone(),
        average_fill_price: result.average_fill_price.clone(),
        average_fee_paid: result.average_fee_paid.clone(),
    })
}

fn correlate<'a, T>(
    items: &'a [T],
    identity: impl Fn(&'a T) -> Option<&'a str>,
) -> HashMap<&'a str, Option<&'a T>> {
    let mut results = HashMap::with_capacity(items.len());
    for item in items {
        if let Some(id) = identity(item) {
            results
                .entry(id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(item));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::execution::submit::tests::pending;

    #[rstest]
    #[case::whole_request("whole")]
    #[case::missing_item("missing")]
    #[case::duplicate_item("duplicate")]
    fn ambiguous_batch_invalidates_readiness_until_account_recovery(#[case] mode: &str) {
        let (submission, mut output) = pending();
        submission.ready.store(true, Ordering::Release);

        let ticket = {
            let mut state = submission.state.lock().unwrap();
            state.epoch = Some((3, 4));
            state.ledger.begin_refresh()
        };

        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut response: BatchCreateResponse =
            serde_json::from_value(fixtures["batch_create"].clone()).unwrap();
        response.orders.truncate(1);
        response.orders[0].client_order_id =
            Field::Value(submission.order.client_order_id().to_string());

        let response = match mode {
            "whole" => Err(KalshiHttpError::Status(503)),
            "missing" => {
                response.orders.clear();
                Ok(response)
            }
            "duplicate" => {
                response.orders.push(response.orders[0].clone());
                Ok(response)
            }
            _ => unreachable!(),
        };

        let chunk = [submission];

        let complete = resolve_submissions(&chunk, response);

        let (epoch, refresh_valid) = {
            let state = chunk[0].state.lock().unwrap();
            (state.epoch, state.ledger.check_refresh(ticket).is_ok())
        };

        assert!(!complete);
        assert!(!chunk[0].ready.load(Ordering::Acquire));
        assert_eq!(epoch, None);
        assert!(!refresh_valid);
        assert!(output.try_recv().is_err());
    }
}

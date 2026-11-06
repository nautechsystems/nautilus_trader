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

//! Application of confirmed managed position operation effects.

use std::str::FromStr;

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use indexmap::IndexSet;
use nautilus_common::{
    cache::Cache,
    messages::execution::{
        PositionOperationControlRequest, PositionOperationEffects, PositionOperationRecovery,
        PositionOperationRecoveryLeg, PositionOperationRecoveryOutcome, PositionOperationResult,
        PositionOperationStartupRecovery, PositionOperationStatus, PositionOperationTokenAmount,
        PositionOperationTokenPayout, QueryAccount,
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    ctf::U256,
    enums::{OmsType, OrderSide},
    events::{PositionChanged, PositionClosed, PositionEvent, PositionOpened},
    identifiers::{AccountId, InstrumentId, PositionId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    position::{Position, PositionReplayEvent},
    position_operation::{PositionOperationDirection, PositionOperationLeg, PositionOperationType},
    types::Quantity,
};
use rust_decimal::Decimal;

use super::ExecutionEngine;

/// Local book inputs checked again once the complete operation barrier is installed.
#[derive(Debug)]
struct BarrierInputs {
    strategy_id: StrategyId,
    account_id: AccountId,
    quantity: Option<Quantity>,
    instrument_ids: Vec<InstrumentId>,
}

#[derive(Debug)]
struct OperationLegPlan {
    instrument: InstrumentAny,
    prior: Option<Position>,
    leg: PositionOperationLeg,
}

impl ExecutionEngine {
    /// Processes an internal adapter-to-core control request.
    pub(crate) fn process_position_operation_control(
        &mut self,
        request: &PositionOperationControlRequest,
    ) {
        match request {
            PositionOperationControlRequest::Barrier {
                operation_id,
                operation,
                instrument_ids,
                strategy_id,
                account_id,
                quantity,
                input_instrument_ids,
                reply,
            } => {
                let inputs = BarrierInputs {
                    strategy_id: *strategy_id,
                    account_id: *account_id,
                    quantity: *quantity,
                    instrument_ids: input_instrument_ids.clone(),
                };
                let result = self.establish_full_barrier(
                    *operation_id,
                    *operation,
                    instrument_ids,
                    Some(&inputs),
                );
                (reply)(result.map_err(|e| e.to_string()));
            }
            PositionOperationControlRequest::Record { key, record, reply } => {
                let result = self.save_position_operation_record(key, record);
                (reply)(result.map_err(|e| e.to_string()));
            }
            PositionOperationControlRequest::Recover { recovery, reply } => {
                let outcome = self
                    .reconcile_recovered_position_operation(recovery)
                    .unwrap_or_else(|e| PositionOperationRecoveryOutcome::Unresolved {
                        message: e.to_string(),
                    });
                (reply)(Ok(outcome));
            }
        }
    }

    /// Restores barriers for unresolved managed operations loaded from the persistent cache.
    ///
    /// Only the account-level barrier scope saved with each record is used. Applied, rejected, or
    /// definitively failed operations no longer block the account.
    pub(crate) fn restore_position_operation_barriers(&self) {
        const PREFIX: &str = "position-operation:";

        let entries = self.cache.borrow().general_entries_with_prefix(PREFIX);
        for (key, bytes) in entries {
            let value: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Failed to read position operation record `{key}`: {e}");
                    continue;
                }
            };

            let application_status = value
                .get("application_status")
                .and_then(serde_json::Value::as_str);
            let venue_status = value
                .get("venue_status")
                .and_then(serde_json::Value::as_str);
            if matches!(application_status, Some("Rejected"))
                || matches!(venue_status, Some("Failed" | "Invalid"))
            {
                continue;
            }

            let operation_id: UUID4 = match value
                .get("operation_id")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
            {
                Ok(Some(operation_id)) => operation_id,
                Ok(None) => {
                    log::error!("Position operation record `{key}` has no operation identity");
                    continue;
                }
                Err(e) => {
                    log::error!("Invalid position operation identity in `{key}`: {e}");
                    continue;
                }
            };

            let instrument_ids: Vec<InstrumentId> = match value
                .get("barrier_instrument_ids")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
            {
                Ok(Some(instrument_ids)) => instrument_ids,
                Ok(None) => {
                    log::error!("Position operation record `{key}` has no barrier scope");
                    continue;
                }
                Err(e) => {
                    log::error!("Invalid position operation barrier scope in `{key}`: {e}");
                    continue;
                }
            };

            let cached_close = {
                let cache = self.cache.borrow();
                instrument_ids
                    .iter()
                    .filter(|instrument_id| cache.instrument_close(instrument_id).is_some())
                    .copied()
                    .collect::<Vec<_>>()
            };
            {
                let mut barriers = self.position_operation_barriers.borrow_mut();
                for instrument_id in &instrument_ids {
                    barriers
                        .entry(*instrument_id)
                        .or_default()
                        .insert(operation_id);
                }
            }
            self.deferred_instrument_closes
                .borrow_mut()
                .extend(cached_close);
            log::warn!(
                "Restored position operation barrier for unresolved operation {operation_id}"
            );
        }
    }

    /// Prepares adapter-held startup recovery evidence before reconciliation.
    ///
    /// This must run outside an active engine borrow so a configured recovery client may query
    /// its venue asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if any execution client cannot prepare its recovery evidence.
    pub async fn prepare_startup_position_operation_recovery(&mut self) -> anyhow::Result<()> {
        for adapter in self.clients.values_mut() {
            adapter.client.prepare_startup_recovery().await?;
        }
        Ok(())
    }

    /// Reconciles prepared startup recoveries before ordinary venue reconciliation.
    pub fn apply_startup_position_operation_recovery(&mut self) {
        let mut prepared = Vec::new();
        for adapter in self.clients.values_mut() {
            prepared.push((adapter.client_id, adapter.client.take_startup_recovery()));
        }

        for (client_id, items) in prepared {
            for item in items {
                match item {
                    PositionOperationStartupRecovery::Reconcile(recovery) => {
                        let outcome = self.reconcile_recovered_position_operation(&recovery);
                        let message = match outcome {
                            Ok(PositionOperationRecoveryOutcome::Applied { .. }) => continue,
                            Ok(PositionOperationRecoveryOutcome::Unresolved { message }) => message,
                            Err(e) => e.to_string(),
                        };
                        if let Some(adapter) = self.clients.get(&client_id) {
                            adapter.client.on_position_operation_recovery_unresolved(
                                recovery.operation_id,
                                PositionOperationStatus::Confirmed,
                                recovery.venue_transaction_id.as_deref(),
                                recovery.transaction_hash.as_deref(),
                                &message,
                            );
                        }
                    }
                    PositionOperationStartupRecovery::Unresolved(unresolved) => {
                        if let Some(adapter) = self.clients.get(&client_id) {
                            adapter.client.on_position_operation_recovery_unresolved(
                                unresolved.operation_id,
                                unresolved.venue_status,
                                unresolved.transaction_id.as_deref(),
                                unresolved.transaction_hash.as_deref(),
                                &unresolved.message,
                            );
                        }
                    }
                    PositionOperationStartupRecovery::Failed(failed) => {
                        self.release_position_operation_barrier(
                            failed.operation_id,
                            &failed.instrument_ids,
                        );
                        if let Some(adapter) = self.clients.get(&client_id) {
                            adapter.client.on_position_operation_recovery_failed(
                                failed.operation_id,
                                failed.transaction_id.as_deref(),
                                failed.transaction_hash.as_deref(),
                                &failed.message,
                            );
                        }
                    }
                }
            }
        }
    }

    /// Reconciles a recovered operation record with persisted position replay state.
    ///
    /// Existing legs are compared by stable identity and exact accounting values. Missing legs
    /// are completed only when the original allocation is fully determined. No position event is
    /// published for legs that replay already contains.
    fn reconcile_recovered_position_operation(
        &mut self,
        recovery: &PositionOperationRecovery,
    ) -> anyhow::Result<PositionOperationRecoveryOutcome> {
        anyhow::ensure!(
            !recovery.instrument_ids.is_empty(),
            "position operation recovery has no barrier scope"
        );

        let existing = self.operation_replay_legs(recovery)?;

        if existing.is_empty()
            && recovery.applied_legs.is_empty()
            && self.applied_settled_legs(recovery).is_empty()
        {
            let Some(effects) = recovery.effects.as_ref() else {
                return Ok(PositionOperationRecoveryOutcome::Unresolved {
                    message: "no verified effects or recorded legs are available".into(),
                });
            };
            let result = self.recovery_result(recovery, Some(effects));

            return match self.apply_position_operation(&result, effects)? {
                Some(legs) => {
                    self.finish_recovered_operation(recovery, &legs, &result);
                    Ok(PositionOperationRecoveryOutcome::Applied {
                        legs: recovery_legs(&legs),
                    })
                }
                None => Ok(PositionOperationRecoveryOutcome::Unresolved {
                    message: "verified effects do not determine complete operation legs".into(),
                }),
            };
        }

        let expected = if recovery.applied_legs.is_empty() {
            let effects = recovery.effects.as_ref().ok_or_else(|| {
                anyhow::anyhow!("partial replay legs require verified effects for recovery")
            })?;
            self.expected_from_effects(recovery, effects, &existing)?
        } else {
            expected_from_record(recovery)?
        };

        let replay_expected = expected
            .iter()
            .filter(|leg| !leg.settled)
            .cloned()
            .collect::<Vec<_>>();
        let settled_expected = expected
            .iter()
            .filter(|leg| leg.settled)
            .cloned()
            .collect::<Vec<_>>();

        let expected_keys: AHashSet<_> = replay_expected
            .iter()
            .map(|leg| {
                (
                    recovery.operation_id,
                    leg.direction,
                    leg.instrument_id,
                    leg.position_id,
                )
            })
            .collect();
        anyhow::ensure!(
            expected_keys.len() == replay_expected.len(),
            "operation recovery requires unique leg identities"
        );
        anyhow::ensure!(
            existing.keys().all(|key| expected_keys.contains(key)),
            "operation replay contains legs outside the recorded allocation"
        );

        let mut missing = Vec::new();
        for leg in &replay_expected {
            let key = (
                recovery.operation_id,
                leg.direction,
                leg.instrument_id,
                leg.position_id,
            );
            match existing.get(&key) {
                Some(replayed) => validate_recovery_leg(recovery, leg, replayed)?,
                None => missing.push(leg.clone()),
            }
        }

        for leg in &settled_expected {
            let consumed = {
                let cache = self.cache.borrow();
                let cycle_opened = cache
                    .position(&leg.position_id)
                    .map_or(nautilus_core::UnixNanos::default(), |position| {
                        position.ts_opened
                    });
                settled_operation_consumed(
                    &cache,
                    recovery.operation_id,
                    leg.position_id,
                    cycle_opened,
                )
            };
            anyhow::ensure!(
                consumed >= leg.quantity.as_decimal(),
                "settled payout for {} has no verified consumption evidence",
                leg.instrument_id
            );
        }

        let settled_legs = settled_expected
            .iter()
            .map(|leg| expected_settled_leg(recovery, leg, self.clock.borrow().timestamp_ns()))
            .collect::<Vec<_>>();

        if missing.is_empty() {
            let mut legs = Vec::with_capacity(replay_expected.len() + settled_legs.len());
            for leg in &replay_expected {
                let key = (
                    recovery.operation_id,
                    leg.direction,
                    leg.instrument_id,
                    leg.position_id,
                );
                legs.push(
                    existing
                        .get(&key)
                        .cloned()
                        .expect("every expected leg is present"),
                );
            }
            legs.extend(settled_legs);

            let result = self.recovery_result(recovery, recovery.effects.as_ref());
            self.finish_recovered_operation(recovery, &legs, &result);
            return Ok(PositionOperationRecoveryOutcome::Applied {
                legs: recovery_legs(&legs),
            });
        }

        for leg in &missing {
            anyhow::ensure!(
                leg.cost.is_some() && leg.income_known,
                "cannot reconstruct missing {} leg for {} without recorded allocation",
                leg.direction,
                leg.instrument_id
            );
        }

        let plans = self.recovery_leg_plans(recovery, &missing)?;
        self.commit_operation_legs(plans)?;

        let refreshed = self.operation_replay_legs(recovery)?;
        anyhow::ensure!(
            refreshed.len() == replay_expected.len(),
            "completed recovery did not produce every expected leg"
        );

        let mut legs = Vec::with_capacity(replay_expected.len() + settled_legs.len());
        for leg in &replay_expected {
            let key = (
                recovery.operation_id,
                leg.direction,
                leg.instrument_id,
                leg.position_id,
            );
            let replayed = refreshed.get(&key).ok_or_else(|| {
                anyhow::anyhow!(
                    "completed recovery is missing {} leg for {}",
                    leg.direction,
                    leg.instrument_id
                )
            })?;
            validate_recovery_leg(recovery, leg, replayed)?;
            legs.push(replayed.clone());
        }
        legs.extend(settled_legs);

        let result = self.recovery_result(recovery, recovery.effects.as_ref());
        self.finish_recovered_operation(recovery, &legs, &result);
        Ok(PositionOperationRecoveryOutcome::Applied {
            legs: recovery_legs(&legs),
        })
    }

    fn recovery_result(
        &self,
        recovery: &PositionOperationRecovery,
        effects: Option<&PositionOperationEffects>,
    ) -> PositionOperationResult {
        let ts = self.clock.borrow().timestamp_ns();
        PositionOperationResult::new(
            recovery.trader_id,
            recovery.strategy_id,
            recovery.client_id,
            recovery.account_id,
            recovery.operation_id,
            recovery.operation,
            PositionOperationStatus::Confirmed,
            recovery.instrument_ids.clone(),
            recovery.venue_transaction_id.clone(),
            recovery.transaction_hash.clone(),
            None,
            effects.cloned(),
            UUID4::new(),
            ts,
            ts,
        )
    }

    fn finish_recovered_operation(
        &mut self,
        recovery: &PositionOperationRecovery,
        legs: &[PositionOperationLeg],
        result: &PositionOperationResult,
    ) {
        self.applied_position_operations
            .insert(recovery.operation_id);
        self.release_position_operation_barrier(recovery.operation_id, &recovery.instrument_ids);
        self.confirm_position_operation(result, legs);
    }

    fn operation_replay_legs(
        &self,
        recovery: &PositionOperationRecovery,
    ) -> anyhow::Result<
        AHashMap<
            (UUID4, PositionOperationDirection, InstrumentId, PositionId),
            PositionOperationLeg,
        >,
    > {
        let mut legs = AHashMap::new();
        let cache = self.cache.borrow();

        for position in cache.positions(None, None, None, None, None) {
            for event in &position.replay_events {
                let PositionReplayEvent::Operation(leg) = event else {
                    continue;
                };
                if leg.operation_id != recovery.operation_id {
                    continue;
                }
                anyhow::ensure!(
                    leg.operation == recovery.operation,
                    "operation replay leg type does not match its record"
                );
                anyhow::ensure!(
                    leg.trader_id == recovery.trader_id && leg.account_id == recovery.account_id,
                    "operation replay leg belongs to a different owner"
                );

                let key = leg.identity();
                if let Some(previous) = legs.insert(key, leg.clone()) {
                    anyhow::ensure!(
                        previous == *leg,
                        "operation replay contains conflicting duplicate legs"
                    );
                }
            }
        }
        Ok(legs)
    }

    fn applied_settled_legs(
        &self,
        recovery: &PositionOperationRecovery,
    ) -> Vec<(PositionId, InstrumentId, StrategyId, Quantity)> {
        let prefix = format!("{SETTLED_CONSUMPTION_PREFIX}:{}:", recovery.operation_id);
        let entries = self.cache.borrow().general_entries_with_prefix(&prefix);
        let cache = self.cache.borrow();
        let mut legs = Vec::new();
        for (key, bytes) in entries {
            let mut parts = key.rsplitn(3, ':');
            let Some(cycle) = parts.next() else {
                continue;
            };
            let Some(position_id) = parts.next() else {
                continue;
            };
            let position_id = PositionId::from(position_id);
            let Some(position) = cache.position(&position_id) else {
                continue;
            };
            let Ok(cycle) = nautilus_core::UnixNanos::from_str(cycle) else {
                continue;
            };
            if position.ts_opened != cycle {
                continue;
            }
            let Some(value) = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|value| Decimal::from_str(value).ok())
            else {
                continue;
            };
            let Ok(quantity) = Quantity::from_decimal_dp(value, position.size_precision) else {
                continue;
            };
            legs.push((
                position_id,
                position.instrument_id,
                position.strategy_id,
                quantity,
            ));
        }
        legs
    }

    fn expected_from_effects(
        &self,
        recovery: &PositionOperationRecovery,
        effects: &PositionOperationEffects,
        existing: &AHashMap<
            (UUID4, PositionOperationDirection, InstrumentId, PositionId),
            PositionOperationLeg,
        >,
    ) -> anyhow::Result<Vec<ExpectedOperationLeg>> {
        let tokens = self.token_instrument_map(&recovery.instrument_ids);
        let mut received = resolve_effects(&effects.received, &tokens)?;
        let mut spent = resolve_effects(&effects.spent, &tokens)?;
        let collateral = Decimal::from_str(&effects.collateral_delta)
            .context("invalid collateral delta in recovered operation effects")?;

        match recovery.operation {
            PositionOperationType::Split => {
                anyhow::ensure!(
                    received.len() == 2,
                    "split recovery requires two outcome credits"
                );
                anyhow::ensure!(
                    !collateral.is_sign_positive(),
                    "split recovery must spend pUSD"
                );
                received.sort_by_key(|(instrument, _)| instrument.id());
                let total_cost = -collateral;
                let mut allocated = Decimal::ZERO;
                let mut legs = Vec::with_capacity(2);
                for (index, (instrument, quantity)) in received.into_iter().enumerate() {
                    let cost = if index == 1 {
                        total_cost - allocated
                    } else {
                        let cost = total_cost / Decimal::from(2u8);
                        allocated += cost;
                        cost
                    };
                    legs.push(ExpectedOperationLeg {
                        position_id: netting_position_id(&instrument.id(), recovery.strategy_id),
                        instrument_id: instrument.id(),
                        strategy_id: recovery.strategy_id,
                        direction: PositionOperationDirection::Open,
                        quantity,
                        cost: Some(cost),
                        income_known: true,
                        income: None,
                        settled: false,
                    });
                }
                Ok(legs)
            }
            PositionOperationType::Merge => {
                anyhow::ensure!(
                    spent.len() == 2,
                    "merge recovery requires two outcome debits"
                );
                anyhow::ensure!(
                    !collateral.is_sign_negative(),
                    "merge recovery must receive pUSD"
                );
                spent.sort_by_key(|(instrument, _)| instrument.id());
                let mut allocated = Decimal::ZERO;
                let mut legs = Vec::with_capacity(2);
                for (index, (instrument, quantity)) in spent.into_iter().enumerate() {
                    let income = if index == 1 {
                        collateral - allocated
                    } else {
                        let income = collateral / Decimal::from(2u8);
                        allocated += income;
                        income
                    };
                    legs.push(ExpectedOperationLeg {
                        position_id: self.recovery_close_position(
                            recovery,
                            &instrument.id(),
                            existing,
                        )?,
                        instrument_id: instrument.id(),
                        strategy_id: recovery.strategy_id,
                        direction: PositionOperationDirection::Close,
                        quantity,
                        cost: None,
                        income_known: true,
                        income: Some(income),
                        settled: false,
                    });
                }
                Ok(legs)
            }
            PositionOperationType::Redeem => {
                anyhow::ensure!(
                    !spent.is_empty(),
                    "redeem recovery requires at least one spent token"
                );
                anyhow::ensure!(
                    !collateral.is_sign_negative(),
                    "redeem payout must be non-negative"
                );

                let mut payout_map = AHashMap::new();
                for payout in &effects.payouts {
                    let numerator = U256::from_str_radix(&payout.numerator, 10)
                        .map_err(|_| anyhow::anyhow!("invalid payout numerator"))?;
                    let denominator = U256::from_str_radix(&payout.denominator, 10)
                        .map_err(|_| anyhow::anyhow!("invalid payout denominator"))?;
                    anyhow::ensure!(
                        !denominator.is_zero() && numerator <= denominator,
                        "invalid CTF payout evidence"
                    );
                    payout_map.insert(
                        ustr::Ustr::from(payout.token_id.as_str()),
                        (numerator, denominator),
                    );
                }

                let spent_len = spent.len();

                if spent_len == 1 && payout_map.is_empty() {
                    let (instrument, quantity) = spent.into_iter().next().expect("one spent token");
                    anyhow::ensure!(
                        collateral >= Decimal::ZERO,
                        "redeem payout must be non-negative"
                    );

                    if let Some((position_id, _, strategy_id, settled_quantity)) = self
                        .applied_settled_legs(recovery)
                        .into_iter()
                        .find(|(_, id, _, _)| *id == instrument.id())
                    {
                        return Ok(vec![ExpectedOperationLeg {
                            position_id,
                            instrument_id: instrument.id(),
                            strategy_id,
                            direction: PositionOperationDirection::Close,
                            quantity: settled_quantity,
                            cost: Some(Decimal::ZERO),
                            income_known: true,
                            income: Some(collateral),
                            settled: true,
                        }]);
                    }

                    let position = self.existing_position(
                        &instrument.id(),
                        recovery.strategy_id,
                        recovery.account_id,
                    )?;
                    let Some(position) = position else {
                        anyhow::bail!(
                            "cannot recover redeem leg for {}: no position exists",
                            instrument.id()
                        );
                    };

                    if position.is_closed() {
                        let Some(physical) = settled_physical_quantity(&position) else {
                            anyhow::bail!(
                                "cannot reconstruct settled physical quantity for {}",
                                position.id
                            );
                        };
                        let consumed = {
                            let cache = self.cache.borrow();
                            settled_consumed(&cache, &position)
                        };
                        let remaining = physical.as_decimal() - consumed;
                        anyhow::ensure!(
                            remaining == quantity.as_decimal(),
                            "settled physical quantity for {} does not match the redeem",
                            instrument.id()
                        );
                        let quantity =
                            Quantity::from_decimal_dp(remaining, position.size_precision)
                                .map_err(|e| anyhow::anyhow!("{e}"))?;
                        return Ok(vec![ExpectedOperationLeg {
                            position_id: position.id,
                            instrument_id: instrument.id(),
                            strategy_id: position.strategy_id,
                            direction: PositionOperationDirection::Close,
                            quantity,
                            cost: Some(Decimal::ZERO),
                            income_known: true,
                            income: Some(collateral),
                            settled: true,
                        }]);
                    }

                    return Ok(vec![ExpectedOperationLeg {
                        position_id: position.id,
                        instrument_id: instrument.id(),
                        strategy_id: position.strategy_id,
                        direction: PositionOperationDirection::Close,
                        quantity,
                        cost: None,
                        income_known: true,
                        income: Some(collateral),
                        settled: false,
                    }]);
                }

                let applied_settled = self.applied_settled_legs(recovery);
                let mut total_income = Decimal::ZERO;
                let mut legs = Vec::with_capacity(spent.len());
                for (instrument, quantity) in spent {
                    let token_id = instrument
                        .info()
                        .and_then(|info| info.get_str("token_id"))
                        .map(ustr::Ustr::from)
                        .ok_or_else(|| {
                            anyhow::anyhow!("redeem instrument {} has no token id", instrument.id())
                        })?;
                    let (numerator, denominator) =
                        payout_map.get(&token_id).copied().ok_or_else(|| {
                            anyhow::anyhow!("missing payout evidence for token {}", instrument.id())
                        })?;

                    let replayed: Vec<&PositionOperationLeg> = existing
                        .values()
                        .filter(|leg| {
                            leg.direction == PositionOperationDirection::Close
                                && leg.instrument_id == instrument.id()
                        })
                        .collect();
                    let settled_applied: Vec<_> = applied_settled
                        .iter()
                        .filter(|(_, id, _, _)| *id == instrument.id())
                        .collect();

                    if !replayed.is_empty() || !settled_applied.is_empty() {
                        let token_payout = nautilus_model::ctf::ctf_payout_amount(
                            quantity,
                            numerator,
                            denominator,
                        )?;
                        total_income += token_payout;

                        let mut entries = replayed
                            .iter()
                            .map(|leg| {
                                (
                                    leg.position_id,
                                    leg.strategy_id,
                                    leg.quantity,
                                    Some(*leg),
                                    false,
                                )
                            })
                            .chain(settled_applied.iter().map(
                                |(position_id, _, strategy_id, quantity)| {
                                    (*position_id, *strategy_id, *quantity, None, true)
                                },
                            ))
                            .collect::<Vec<_>>();
                        entries.sort_by_key(|(position_id, _, _, _, _)| position_id.to_string());
                        let weights = entries
                            .iter()
                            .map(|(_, _, quantity, _, _)| quantity.as_decimal())
                            .collect::<Vec<_>>();
                        let incomes = allocate(token_payout, &weights);

                        for ((position_id, strategy_id, quantity, leg, settled), income) in
                            entries.into_iter().zip(incomes)
                        {
                            if let Some(leg) = leg {
                                anyhow::ensure!(
                                    leg.income == Some(income),
                                    "replayed CTF redeem income for {} does not match its payout",
                                    instrument.id()
                                );
                                legs.push(ExpectedOperationLeg {
                                    position_id,
                                    instrument_id: instrument.id(),
                                    strategy_id,
                                    direction: PositionOperationDirection::Close,
                                    quantity,
                                    cost: Some(leg.cost),
                                    income_known: true,
                                    income: Some(income),
                                    settled,
                                });
                            } else {
                                legs.push(ExpectedOperationLeg {
                                    position_id,
                                    instrument_id: instrument.id(),
                                    strategy_id,
                                    direction: PositionOperationDirection::Close,
                                    quantity,
                                    cost: Some(Decimal::ZERO),
                                    income_known: true,
                                    income: Some(income),
                                    settled,
                                });
                            }
                        }
                        continue;
                    }

                    let (mut positions, settled_positions) = {
                        let cache = self.cache.borrow();
                        let positions = cache
                            .positions_open(
                                None,
                                Some(&instrument.id()),
                                None,
                                Some(&recovery.account_id),
                                None,
                            )
                            .into_iter()
                            .filter(|position| position.is_long())
                            .map(|position| position.cloned())
                            .collect::<Vec<_>>();

                        let mut settled_positions = Vec::new();
                        for position in cache.positions(
                            None,
                            Some(&instrument.id()),
                            None,
                            Some(&recovery.account_id),
                            None,
                        ) {
                            if !position.is_closed() {
                                continue;
                            }
                            let Some(physical) = settled_physical_quantity(&position) else {
                                anyhow::ensure!(
                                    !position.replay_events.iter().any(|event| matches!(
                                        event,
                                        PositionReplayEvent::InstrumentClosed(_)
                                    )),
                                    "cannot reconstruct settled physical quantity for {}",
                                    position.id
                                );
                                continue;
                            };
                            let consumed = settled_consumed(&cache, &position);
                            let remaining = physical.as_decimal() - consumed;
                            if remaining > Decimal::ZERO {
                                settled_positions.push((position.cloned(), remaining));
                            }
                        }
                        (positions, settled_positions)
                    };
                    positions.sort_by_key(|position| position.id.to_string());

                    let open_available: Decimal = positions
                        .iter()
                        .map(|position| position.quantity.as_decimal())
                        .sum();
                    let settled_available: Decimal = settled_positions
                        .iter()
                        .map(|(_, quantity)| *quantity)
                        .sum();
                    anyhow::ensure!(
                        !positions.is_empty() || !settled_positions.is_empty(),
                        "no known positions explain the CTF redeem balance for {}",
                        instrument.id()
                    );
                    anyhow::ensure!(
                        open_available + settled_available == quantity.as_decimal(),
                        "known positions do not fully explain the CTF redeem balance for {}",
                        instrument.id()
                    );

                    let token_payout =
                        nautilus_model::ctf::ctf_payout_amount(quantity, numerator, denominator)?;
                    total_income += token_payout;

                    let mut entries = positions
                        .into_iter()
                        .map(|position| (position, false))
                        .chain(
                            settled_positions
                                .into_iter()
                                .map(|(position, _)| (position, true)),
                        )
                        .collect::<Vec<_>>();
                    entries.sort_by_key(|(position, _)| position.id.to_string());
                    let weights = entries
                        .iter()
                        .map(|(position, settled)| {
                            if *settled {
                                let physical =
                                    settled_physical_quantity(position).unwrap_or_default();
                                physical.as_decimal()
                                    - settled_consumed(&self.cache.borrow(), position)
                            } else {
                                position.quantity.as_decimal()
                            }
                        })
                        .collect::<Vec<_>>();
                    let incomes = allocate(token_payout, &weights);

                    for ((position, settled), income) in entries.into_iter().zip(incomes) {
                        let quantity = if settled {
                            let physical =
                                settled_physical_quantity(&position).ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "cannot reconstruct settled physical quantity for {}",
                                        position.id
                                    )
                                })?;
                            let remaining = physical.as_decimal()
                                - settled_consumed(&self.cache.borrow(), &position);
                            Quantity::from_decimal_dp(remaining, position.size_precision)
                                .map_err(|e| anyhow::anyhow!("{e}"))?
                        } else {
                            position.quantity
                        };
                        legs.push(ExpectedOperationLeg {
                            position_id: position.id,
                            instrument_id: instrument.id(),
                            strategy_id: position.strategy_id,
                            direction: PositionOperationDirection::Close,
                            quantity,
                            cost: if settled { Some(Decimal::ZERO) } else { None },
                            income_known: true,
                            income: Some(income),
                            settled,
                        });
                    }
                }
                anyhow::ensure!(
                    total_income == collateral,
                    "recovered CTF payout allocation does not match the collateral return"
                );
                Ok(legs)
            }
            PositionOperationType::Convert => {
                anyhow::ensure!(
                    !spent.is_empty(),
                    "convert recovery requires NO input tokens"
                );
                anyhow::ensure!(
                    !collateral.is_sign_negative(),
                    "convert collateral return must be non-negative"
                );
                spent.sort_by_key(|(instrument, _)| instrument.id());
                received.sort_by_key(|(instrument, _)| instrument.id());

                let mut inputs = Vec::with_capacity(spent.len());
                let mut consumed_cost = Decimal::ZERO;
                let mut all_inputs_present = true;
                for (instrument, quantity) in &spent {
                    let position_id =
                        self.recovery_close_position(recovery, &instrument.id(), existing)?;
                    let key = (
                        recovery.operation_id,
                        PositionOperationDirection::Close,
                        instrument.id(),
                        position_id,
                    );
                    let cost = existing.get(&key).map(|leg| leg.cost);
                    if let Some(cost) = cost {
                        consumed_cost += cost;
                    } else {
                        all_inputs_present = false;
                    }
                    inputs.push((instrument.id(), *quantity, position_id, cost));
                }

                let has_outputs = !received.is_empty();
                let (immediate_pnl, output_cost) = if all_inputs_present {
                    let immediate_pnl = if has_outputs {
                        (collateral - consumed_cost).max(Decimal::ZERO)
                    } else {
                        collateral - consumed_cost
                    };
                    let output_cost = if has_outputs {
                        (consumed_cost - collateral).max(Decimal::ZERO)
                    } else {
                        Decimal::ZERO
                    };
                    (Some(immediate_pnl), Some(output_cost))
                } else {
                    (None, None)
                };

                let pnl_shares = immediate_pnl.map(|pnl| {
                    let weights: Vec<Decimal> = if consumed_cost.is_zero() {
                        inputs
                            .iter()
                            .map(|(_, quantity, _, _)| quantity.as_decimal())
                            .collect()
                    } else {
                        inputs
                            .iter()
                            .map(|(_, _, _, cost)| cost.unwrap_or(Decimal::ZERO))
                            .collect()
                    };
                    allocate(pnl, &weights)
                });

                let mut legs = Vec::with_capacity(inputs.len() + received.len());
                for (index, (instrument_id, quantity, position_id, cost)) in
                    inputs.into_iter().enumerate()
                {
                    let income = match (cost, &pnl_shares) {
                        (Some(cost), Some(shares)) => Some(cost + shares[index]),
                        _ => None,
                    };
                    legs.push(ExpectedOperationLeg {
                        position_id,
                        instrument_id,
                        strategy_id: recovery.strategy_id,
                        direction: PositionOperationDirection::Close,
                        quantity,
                        cost,
                        income_known: income.is_some(),
                        income,
                        settled: false,
                    });
                }

                let output_costs = output_cost.map(|cost| {
                    let weights: Vec<Decimal> = received
                        .iter()
                        .map(|(_, quantity)| quantity.as_decimal())
                        .collect();
                    allocate(cost, &weights)
                });

                for (index, (instrument, quantity)) in received.into_iter().enumerate() {
                    legs.push(ExpectedOperationLeg {
                        position_id: netting_position_id(&instrument.id(), recovery.strategy_id),
                        instrument_id: instrument.id(),
                        strategy_id: recovery.strategy_id,
                        direction: PositionOperationDirection::Open,
                        quantity,
                        cost: output_costs.as_ref().map(|costs| costs[index]),
                        income_known: true,
                        income: None,
                        settled: false,
                    });
                }
                Ok(legs)
            }
        }
    }

    fn recovery_close_position(
        &self,
        recovery: &PositionOperationRecovery,
        instrument_id: &InstrumentId,
        existing: &AHashMap<
            (UUID4, PositionOperationDirection, InstrumentId, PositionId),
            PositionOperationLeg,
        >,
    ) -> anyhow::Result<PositionId> {
        if let Some(leg) = existing.values().find(|leg| {
            leg.direction == PositionOperationDirection::Close
                && leg.instrument_id == *instrument_id
        }) {
            return Ok(leg.position_id);
        }

        self.existing_position(instrument_id, recovery.strategy_id, recovery.account_id)?
            .map(|position| position.id)
            .ok_or_else(|| {
                anyhow::anyhow!("cannot recover close leg for {instrument_id}: no position exists")
            })
    }

    fn recovery_leg_plans(
        &self,
        recovery: &PositionOperationRecovery,
        missing: &[ExpectedOperationLeg],
    ) -> anyhow::Result<Vec<OperationLegPlan>> {
        let ts = self.clock.borrow().timestamp_ns();
        let mut plans = Vec::with_capacity(missing.len());
        let cache = self.cache.borrow();

        for leg in missing {
            let instrument = cache
                .instrument(&leg.instrument_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cannot recover operation leg: instrument {} is not in the cache",
                        leg.instrument_id
                    )
                })?;
            let prior = cache
                .position(&leg.position_id)
                .map(|position| position.clone());
            anyhow::ensure!(
                leg.direction == PositionOperationDirection::Open || prior.is_some(),
                "cannot recover close leg for {}: position {} is missing",
                leg.instrument_id,
                leg.position_id
            );

            let (trader_id, strategy_id, account_id) = prior.as_ref().map_or(
                (recovery.trader_id, leg.strategy_id, recovery.account_id),
                |position| {
                    (
                        position.trader_id,
                        position.strategy_id,
                        position.account_id,
                    )
                },
            );
            let position_leg = PositionOperationLeg {
                operation_id: recovery.operation_id,
                operation: recovery.operation,
                direction: leg.direction,
                trader_id,
                strategy_id,
                account_id,
                instrument_id: leg.instrument_id,
                position_id: leg.position_id,
                quantity: leg.quantity,
                cost: leg.cost.expect("missing leg cost was validated"),
                income: leg.income,
                settled: false,
                event_id: UUID4::new(),
                ts_event: ts,
                ts_init: ts,
            };

            plans.push(OperationLegPlan {
                instrument,
                prior,
                leg: position_leg,
            });
        }
        Ok(plans)
    }

    fn establish_full_barrier(
        &self,
        operation_id: UUID4,
        operation: PositionOperationType,
        instrument_ids: &[InstrumentId],
        inputs: Option<&BarrierInputs>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !instrument_ids.is_empty(),
            "position operation barrier requires at least one instrument"
        );

        if let Some(inputs) = inputs {
            anyhow::ensure!(
                inputs
                    .instrument_ids
                    .iter()
                    .all(|instrument_id| instrument_ids.contains(instrument_id)),
                "position operation inputs must be inside the barrier scope"
            );
        }

        if operation != PositionOperationType::Redeem {
            let cache = self.cache.borrow();
            for instrument_id in instrument_ids {
                if cache.instrument_close(instrument_id).is_some_and(|close| {
                    close.close_type == nautilus_model::enums::InstrumentCloseType::ContractExpired
                }) {
                    anyhow::bail!("instrument {instrument_id} is already economically settled");
                }
            }
        }

        {
            let barriers = self.position_operation_barriers.borrow();
            for instrument_id in instrument_ids {
                if let Some(existing) = barriers
                    .get(instrument_id)
                    .and_then(|set| set.iter().next())
                {
                    anyhow::bail!(
                        "instrument {instrument_id} is barred by position operation {existing}"
                    );
                }
            }
        }

        {
            let cache = self.cache.borrow();
            for instrument_id in instrument_ids {
                if !cache
                    .orders_open(None, Some(instrument_id), None, None, None)
                    .is_empty()
                {
                    anyhow::bail!("instrument {instrument_id} has active CLOB orders");
                }
            }
        }

        {
            let mut barriers = self.position_operation_barriers.borrow_mut();
            for instrument_id in instrument_ids {
                barriers
                    .entry(*instrument_id)
                    .or_default()
                    .insert(operation_id);
            }
        }

        if let Some(inputs) = inputs
            && let Err(e) = self.ensure_barrier_inputs(operation, inputs)
        {
            self.release_position_operation_barrier(operation_id, instrument_ids);
            return Err(e);
        }

        Ok(())
    }

    /// Rechecks the operation's local book ownership and quantity after the barrier is installed.
    ///
    /// The adapter's earlier check happens before asynchronous metadata and settlement work, so a
    /// CLOB fill can reduce the input position in between. Only a V2 redeem may use the physical
    /// remainder of an economically settled position, and only up to the quantity not yet paid out.
    fn ensure_barrier_inputs(
        &self,
        operation: PositionOperationType,
        inputs: &BarrierInputs,
    ) -> anyhow::Result<()> {
        if operation == PositionOperationType::Split {
            return Ok(());
        }
        if operation == PositionOperationType::Redeem && inputs.instrument_ids.len() == 2 {
            // A CTF redeem is wallet-scoped; ownership is resolved from confirmed effects.
            return Ok(());
        }

        let amount = inputs.quantity.ok_or_else(|| {
            anyhow::anyhow!("position operation barrier requires an explicit quantity")
        })?;
        anyhow::ensure!(
            amount.as_decimal() > Decimal::ZERO,
            "position operation quantity must be positive"
        );

        let cache = self.cache.borrow();
        for instrument_id in &inputs.instrument_ids {
            let open = cache.positions_open(
                None,
                Some(instrument_id),
                Some(&inputs.strategy_id),
                Some(&inputs.account_id),
                None,
            );
            anyhow::ensure!(
                open.len() <= 1,
                "multiple open positions for {instrument_id} in NETTING mode"
            );
            let available = open.first().map_or(Decimal::ZERO, |position| {
                position.signed_decimal_qty().max(Decimal::ZERO)
            });
            if available >= amount.as_decimal() {
                continue;
            }

            anyhow::ensure!(
                operation == PositionOperationType::Redeem,
                "position operation requires {} available {instrument_id}, found {available}",
                amount.as_decimal()
            );
            let position_id = netting_position_id(instrument_id, inputs.strategy_id);
            let position = cache.position(&position_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "position operation requires {} available {instrument_id}, found {available}",
                    amount.as_decimal()
                )
            })?;
            anyhow::ensure!(
                position.account_id == inputs.account_id && position.is_closed(),
                "position operation requires {} available {instrument_id}, found {available}",
                amount.as_decimal()
            );
            let physical = settled_physical_quantity(&position).ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot verify redeemable quantity for settled position {}",
                    position.id
                )
            })?;
            let remaining = physical.as_decimal() - settled_consumed(&cache, &position);
            anyhow::ensure!(
                remaining >= amount.as_decimal(),
                "position operation requires {} redeemable {instrument_id}, found {remaining}",
                amount.as_decimal()
            );
        }

        Ok(())
    }

    fn save_position_operation_record(
        &self,
        key: &str,
        record: &serde_json::Value,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            key.starts_with("position-operation:"),
            "invalid position operation record key"
        );
        let payload = serde_json::to_vec(record)
            .map_err(|e| anyhow::anyhow!("failed to serialize position operation: {e}"))?;
        self.cache
            .borrow_mut()
            .add(key, bytes::Bytes::from(payload))?;
        Ok(())
    }

    /// Releases any barrier held for `operation_id` on the given instruments.
    pub(crate) fn release_position_operation_barrier(
        &self,
        operation_id: UUID4,
        instrument_ids: &[InstrumentId],
    ) {
        let mut released = Vec::new();
        {
            let mut barriers = self.position_operation_barriers.borrow_mut();
            for instrument_id in instrument_ids {
                if let Some(set) = barriers.get_mut(instrument_id)
                    && set.remove(&operation_id)
                {
                    if set.is_empty() {
                        barriers.remove(instrument_id);
                    }
                    released.push(*instrument_id);
                }
            }
        }

        let deferred = {
            let mut deferred = self.deferred_instrument_closes.borrow_mut();
            released
                .into_iter()
                .filter(|instrument_id| deferred.remove(instrument_id))
                .collect::<Vec<_>>()
        };
        for instrument_id in deferred {
            let Some(close) = self
                .cache
                .borrow()
                .instrument_close(&instrument_id)
                .copied()
            else {
                continue;
            };
            self.settle_instrument_close(&close);
        }
    }

    /// Returns whether any managed position operation currently bars `instrument_id`.
    #[must_use]
    pub fn is_position_operation_barred(&self, instrument_id: &InstrumentId) -> bool {
        self.position_operation_barrier(instrument_id).is_some()
    }

    /// Returns every instrument currently barred by an unresolved managed position operation.
    #[must_use]
    pub fn position_operation_barred_instruments(&self) -> IndexSet<InstrumentId> {
        self.position_operation_barriers
            .borrow()
            .keys()
            .copied()
            .collect()
    }

    /// Returns the operation currently barring `instrument_id`.
    pub(crate) fn position_operation_barrier(&self, instrument_id: &InstrumentId) -> Option<UUID4> {
        self.position_operation_barriers
            .borrow()
            .get(instrument_id)
            .and_then(|set| set.iter().next().copied())
    }

    /// Applies confirmed managed position operation effects exactly once.
    pub(crate) fn process_position_operation(&mut self, result: &PositionOperationResult) {
        if matches!(
            result.status,
            PositionOperationStatus::Rejected
                | PositionOperationStatus::Failed
                | PositionOperationStatus::Applied
        ) {
            self.release_position_operation_barrier(result.operation_id, &result.instrument_ids);
        }

        if result.status != PositionOperationStatus::Confirmed {
            return;
        }

        if self
            .applied_position_operations
            .contains(&result.operation_id)
        {
            log::debug!(
                "Ignoring duplicate position operation application {}",
                result.operation_id
            );
            return;
        }

        let Some(effects) = &result.effects else {
            log::warn!(
                "Position operation {} is confirmed without parsed effects; local state remains unresolved",
                result.operation_id
            );
            return;
        };

        match self.apply_position_operation(result, effects) {
            Ok(Some(legs)) => {
                self.applied_position_operations.insert(result.operation_id);
                self.release_position_operation_barrier(
                    result.operation_id,
                    &result.instrument_ids,
                );
                self.confirm_position_operation(result, &legs);
            }
            Ok(None) => {
                log::warn!(
                    "Position operation {} lacks complete ownership or payout evidence; local state remains unresolved",
                    result.operation_id
                );
            }
            Err(e) => {
                log::warn!(
                    "Failed to apply position operation {}: {e}; local state remains unresolved",
                    result.operation_id
                );
            }
        }
    }

    fn confirm_position_operation(
        &self,
        result: &PositionOperationResult,
        legs: &[PositionOperationLeg],
    ) {
        let Some(adapter) = self.clients.get(&result.client_id) else {
            log::warn!(
                "Cannot confirm position operation {}: execution client {} is not registered",
                result.operation_id,
                result.client_id
            );
            return;
        };

        adapter.client.on_position_operation_applied(
            result.operation_id,
            legs,
            result.transaction_hash.as_deref(),
        );

        // Refresh venue balances through the normal account-state path so the applied token and
        // collateral effects reach the account without adding a second cash ledger.
        let query = QueryAccount::new(
            result.trader_id,
            Some(result.client_id),
            result.account_id,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            None,
            Some(result.operation_id),
        );
        self.handle_query_account(adapter.client.as_ref(), query);
    }

    fn apply_position_operation(
        &self,
        result: &PositionOperationResult,
        effects: &PositionOperationEffects,
    ) -> anyhow::Result<Option<Vec<PositionOperationLeg>>> {
        if result.operation != PositionOperationType::Redeem {
            let deferred = self.deferred_instrument_closes.borrow();
            let cache = self.cache.borrow();
            for instrument_id in &result.instrument_ids {
                if cache.instrument_close(instrument_id).is_some_and(|close| {
                    close.close_type == nautilus_model::enums::InstrumentCloseType::ContractExpired
                }) && !deferred.contains(instrument_id)
                {
                    log::warn!(
                        "Position operation {} cannot apply to settled instrument {instrument_id}",
                        result.operation_id
                    );
                    return Ok(None);
                }
            }
        }

        let tokens = self.token_instrument_map(&result.instrument_ids);
        let received = resolve_effects(&effects.received, &tokens)?;
        let spent = resolve_effects(&effects.spent, &tokens)?;
        let collateral = Decimal::from_str(&effects.collateral_delta)
            .context("invalid collateral delta in position operation effects")?;
        let ts_event = result.ts_event;
        let ts_init = self.clock.borrow().timestamp_ns();

        let (plans, settled) = match result.operation {
            PositionOperationType::Split => (
                self.plan_split(result, received, collateral, ts_event, ts_init)?,
                Vec::new(),
            ),
            PositionOperationType::Merge => (
                self.plan_merge(result, spent, collateral, ts_event, ts_init)?,
                Vec::new(),
            ),
            PositionOperationType::Redeem => {
                match self.plan_redeem(
                    result,
                    spent,
                    &effects.payouts,
                    collateral,
                    ts_event,
                    ts_init,
                )? {
                    Some(redeem) => redeem,
                    None => return Ok(None),
                }
            }
            PositionOperationType::Convert => (
                self.plan_convert(result, received, spent, collateral, ts_event, ts_init)?,
                Vec::new(),
            ),
        };

        let mut legs = self.commit_operation_legs(plans)?;
        if !settled.is_empty() {
            self.record_settled_consumption(&settled)?;
            legs.extend(settled);
        }
        Ok(Some(legs))
    }

    fn record_settled_consumption(&self, legs: &[PositionOperationLeg]) -> anyhow::Result<()> {
        let mut errors = Vec::new();
        for leg in legs.iter().filter(|leg| leg.settled) {
            let cycle_opened = self
                .cache
                .borrow()
                .position(&leg.position_id)
                .map_or(nautilus_core::UnixNanos::default(), |position| {
                    position.ts_opened
                });
            let key = settled_consumption_key(leg.position_id, cycle_opened);
            let current = {
                let cache = self.cache.borrow();
                cache
                    .get(&key)?
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|value| Decimal::from_str(value).ok())
                    .unwrap_or_default()
            };
            let operation_key =
                settled_operation_key(&leg.operation_id, leg.position_id, cycle_opened);
            let operation_current = {
                let cache = self.cache.borrow();
                cache
                    .get(&operation_key)?
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|value| Decimal::from_str(value).ok())
                    .unwrap_or_default()
            };
            let leg_quantity = leg.quantity.as_decimal();
            let remaining = leg_quantity - operation_current;
            if !remaining.is_sign_positive() {
                // This operation already recorded its payout for this position. Cache persists
                // memory before reporting a database error, so a retry must not add it again.
                continue;
            }

            let next = current + remaining;
            let operation_next = operation_current + remaining;
            {
                let mut cache = self.cache.borrow_mut();
                if let Err(e) = cache.add(&key, bytes::Bytes::from(next.to_string())) {
                    errors.push(e.to_string());
                }
                if let Err(e) = cache.add(
                    &operation_key,
                    bytes::Bytes::from(operation_next.to_string()),
                ) {
                    errors.push(e.to_string());
                }
            }
        }

        if !errors.is_empty() {
            log::error!(
                "Settled position operation consumption was applied in memory with {} persistence error(s): {}",
                errors.len(),
                errors.join("; ")
            );
        }
        Ok(())
    }

    fn token_instrument_map(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> AHashMap<ustr::Ustr, InstrumentAny> {
        let mut map = AHashMap::new();
        let Some(venue) = instrument_ids.first().map(|id| id.venue) else {
            return map;
        };

        let cache = self.cache.borrow();
        for instrument in cache.instruments(&venue, None) {
            if !instrument_ids.contains(&instrument.id()) {
                continue;
            }
            if let Some(info) = instrument.info()
                && let Some(token_id) = info.get_str("token_id")
            {
                map.insert(ustr::Ustr::from(token_id), (*instrument).clone());
            }
        }
        map
    }

    fn existing_position(
        &self,
        instrument_id: &InstrumentId,
        strategy_id: StrategyId,
        account_id: AccountId,
    ) -> anyhow::Result<Option<Position>> {
        let cache = self.cache.borrow();
        let open = cache.positions_open(
            None,
            Some(instrument_id),
            Some(&strategy_id),
            Some(&account_id),
            None,
        );

        match open.as_slice() {
            [position] => Ok(Some(position.cloned())),
            [] => {
                let position_id = netting_position_id(instrument_id, strategy_id);
                Ok(cache
                    .position(&position_id)
                    .map(|position| position.cloned()))
            }
            _ => anyhow::bail!("multiple open positions for {instrument_id} in NETTING mode"),
        }
    }

    fn plan_split(
        &self,
        result: &PositionOperationResult,
        mut received: Vec<(InstrumentAny, Quantity)>,
        collateral: Decimal,
        ts_event: nautilus_core::UnixNanos,
        ts_init: nautilus_core::UnixNanos,
    ) -> anyhow::Result<Vec<OperationLegPlan>> {
        anyhow::ensure!(
            received.len() == 2,
            "split requires two outcome token credits"
        );
        anyhow::ensure!(
            !collateral.is_sign_positive(),
            "split must spend pUSD collateral"
        );
        let total_cost = -collateral;
        received.sort_by_key(|(instrument, _)| instrument.id());

        let mut plans = Vec::with_capacity(2);
        let mut allocated = Decimal::ZERO;
        for (index, (instrument, quantity)) in received.into_iter().enumerate() {
            let cost = if index == 1 {
                total_cost - allocated
            } else {
                let cost = total_cost / Decimal::from(2u8);
                allocated += cost;
                cost
            };
            let position_id = netting_position_id(&instrument.id(), result.strategy_id);
            let prior =
                self.existing_position(&instrument.id(), result.strategy_id, result.account_id)?;
            let leg = build_leg(
                result,
                PositionOperationDirection::Open,
                &instrument,
                position_id,
                quantity,
                cost,
                None,
                ts_event,
                ts_init,
            );
            plans.push(OperationLegPlan {
                instrument,
                prior,
                leg,
            });
        }
        Ok(plans)
    }

    fn plan_merge(
        &self,
        result: &PositionOperationResult,
        mut spent: Vec<(InstrumentAny, Quantity)>,
        collateral: Decimal,
        ts_event: nautilus_core::UnixNanos,
        ts_init: nautilus_core::UnixNanos,
    ) -> anyhow::Result<Vec<OperationLegPlan>> {
        anyhow::ensure!(spent.len() == 2, "merge requires two outcome token debits");
        anyhow::ensure!(!collateral.is_sign_negative(), "merge must receive pUSD");
        spent.sort_by_key(|(instrument, _)| instrument.id());

        let mut plans = Vec::with_capacity(2);
        let mut allocated = Decimal::ZERO;
        for (index, (instrument, quantity)) in spent.into_iter().enumerate() {
            let income = if index == 1 {
                collateral - allocated
            } else {
                let income = collateral / Decimal::from(2u8);
                allocated += income;
                income
            };
            let prior = self
                .existing_position(&instrument.id(), result.strategy_id, result.account_id)?
                .ok_or_else(|| {
                    anyhow::anyhow!("merge input position is not open for {}", instrument.id())
                })?;
            let cost = release_cost(&prior, quantity)?;
            let position_id = prior.id;
            let leg = build_leg(
                result,
                PositionOperationDirection::Close,
                &instrument,
                position_id,
                quantity,
                cost,
                Some(income),
                ts_event,
                ts_init,
            );
            plans.push(OperationLegPlan {
                instrument,
                prior: Some(prior),
                leg,
            });
        }
        Ok(plans)
    }

    fn plan_redeem(
        &self,
        result: &PositionOperationResult,
        spent: Vec<(InstrumentAny, Quantity)>,
        payouts: &[PositionOperationTokenPayout],
        collateral: Decimal,
        ts_event: nautilus_core::UnixNanos,
        ts_init: nautilus_core::UnixNanos,
    ) -> anyhow::Result<Option<(Vec<OperationLegPlan>, Vec<PositionOperationLeg>)>> {
        anyhow::ensure!(
            !collateral.is_sign_negative(),
            "redeem payout must be non-negative"
        );

        let mut payout_map = AHashMap::new();
        for payout in payouts {
            let numerator = U256::from_str_radix(&payout.numerator, 10)
                .map_err(|_| anyhow::anyhow!("invalid CTF payout numerator"))?;
            let denominator = U256::from_str_radix(&payout.denominator, 10)
                .map_err(|_| anyhow::anyhow!("invalid CTF payout denominator"))?;
            if denominator.is_zero() || numerator > denominator {
                return Ok(None);
            }
            payout_map.insert(
                ustr::Ustr::from(payout.token_id.as_str()),
                (numerator, denominator),
            );
        }

        if spent.is_empty() {
            return Ok(None);
        }

        // A CTF redeem burns both outcomes. Allocating the aggregate payout requires verified
        // per-outcome payout evidence for every spent token.
        if spent.len() > 1 || !payout_map.is_empty() {
            let mut plans = Vec::new();
            let mut settled_legs = Vec::new();
            let mut total_income = Decimal::ZERO;

            for (instrument, quantity) in spent {
                let token_id = instrument
                    .info()
                    .and_then(|info| info.get_str("token_id"))
                    .map(ustr::Ustr::from)
                    .ok_or_else(|| {
                        anyhow::anyhow!("redeem instrument {} has no token id", instrument.id())
                    })?;
                let Some((numerator, denominator)) = payout_map.get(&token_id).copied() else {
                    return Ok(None);
                };

                let (positions, settled_positions) = {
                    let cache = self.cache.borrow();
                    let mut positions = cache
                        .positions_open(
                            None,
                            Some(&instrument.id()),
                            None,
                            Some(&result.account_id),
                            None,
                        )
                        .into_iter()
                        .filter(|position| position.is_long())
                        .map(|position| position.cloned())
                        .collect::<Vec<_>>();
                    positions.sort_by_key(|position| position.id.to_string());

                    let mut settled_positions = Vec::new();
                    for position in cache.positions(
                        None,
                        Some(&instrument.id()),
                        None,
                        Some(&result.account_id),
                        None,
                    ) {
                        if !position.is_closed() {
                            continue;
                        }
                        let Some(physical) = settled_physical_quantity(&position) else {
                            if position.replay_events.iter().any(|event| {
                                matches!(event, PositionReplayEvent::InstrumentClosed(_))
                            }) {
                                return Ok(None);
                            }
                            continue;
                        };
                        let consumed = settled_consumed(&cache, &position);
                        let remaining = physical.as_decimal() - consumed;
                        if remaining > Decimal::ZERO {
                            settled_positions.push((position.cloned(), remaining));
                        }
                    }
                    settled_positions.sort_by_key(|(position, _)| position.id.to_string());
                    (positions, settled_positions)
                };

                let open_available: Decimal = positions
                    .iter()
                    .map(|position| position.quantity.as_decimal())
                    .sum();
                let settled_available: Decimal = settled_positions
                    .iter()
                    .map(|(_, quantity)| *quantity)
                    .sum();
                if positions.is_empty() && settled_positions.is_empty() {
                    return Ok(None);
                }
                if open_available + settled_available != quantity.as_decimal() {
                    // The wallet balance is not fully explained by known positions.
                    return Ok(None);
                }

                let token_payout =
                    nautilus_model::ctf::ctf_payout_amount(quantity, numerator, denominator)?;
                total_income += token_payout;

                let mut entries = positions
                    .into_iter()
                    .map(|position| (position, false))
                    .chain(
                        settled_positions
                            .into_iter()
                            .map(|(position, _)| (position, true)),
                    )
                    .collect::<Vec<_>>();
                entries.sort_by_key(|(position, _)| position.id.to_string());
                let weights: Vec<Decimal> = entries
                    .iter()
                    .map(|(position, settled)| {
                        if *settled {
                            let physical = settled_physical_quantity(position).unwrap_or_default();
                            physical.as_decimal() - settled_consumed(&self.cache.borrow(), position)
                        } else {
                            position.quantity.as_decimal()
                        }
                    })
                    .collect();
                let incomes = allocate(token_payout, &weights);

                for ((position, settled), income) in entries.into_iter().zip(incomes) {
                    if settled {
                        let physical = settled_physical_quantity(&position).ok_or_else(|| {
                            anyhow::anyhow!(
                                "cannot reconstruct settled physical quantity for {}",
                                position.id
                            )
                        })?;
                        let remaining = physical.as_decimal()
                            - settled_consumed(&self.cache.borrow(), &position);
                        let quantity =
                            Quantity::from_decimal_dp(remaining, position.size_precision)
                                .map_err(|e| anyhow::anyhow!("{e}"))?;
                        settled_legs.push(PositionOperationLeg {
                            operation_id: result.operation_id,
                            operation: result.operation,
                            direction: PositionOperationDirection::Close,
                            trader_id: position.trader_id,
                            strategy_id: position.strategy_id,
                            account_id: position.account_id,
                            instrument_id: instrument.id(),
                            position_id: position.id,
                            quantity,
                            cost: Decimal::ZERO,
                            income: Some(income),
                            settled: true,
                            event_id: UUID4::new(),
                            ts_event,
                            ts_init,
                        });
                    } else {
                        let position_quantity = position.quantity;
                        let cost = release_cost(&position, position_quantity)?;
                        let leg = build_position_leg(
                            result,
                            PositionOperationDirection::Close,
                            &instrument,
                            &position,
                            position_quantity,
                            cost,
                            Some(income),
                            ts_event,
                            ts_init,
                        );
                        plans.push(OperationLegPlan {
                            instrument: instrument.clone(),
                            prior: Some(position),
                            leg,
                        });
                    }
                }
            }

            anyhow::ensure!(
                total_income == collateral,
                "CTF payout allocation does not match the actual collateral return"
            );
            return Ok(Some((plans, settled_legs)));
        }

        let (instrument, quantity) = spent.into_iter().next().expect("one spent token");
        let Some(prior) =
            self.existing_position(&instrument.id(), result.strategy_id, result.account_id)?
        else {
            return Ok(None);
        };
        if prior.is_closed() {
            let Some(physical) = settled_physical_quantity(&prior) else {
                return Ok(None);
            };
            let consumed = {
                let cache = self.cache.borrow();
                settled_consumed(&cache, &prior)
            };
            let remaining = physical.as_decimal() - consumed;
            if quantity.as_decimal() > remaining {
                return Ok(None);
            }
            let leg = PositionOperationLeg {
                operation_id: result.operation_id,
                operation: result.operation,
                direction: PositionOperationDirection::Close,
                trader_id: prior.trader_id,
                strategy_id: prior.strategy_id,
                account_id: prior.account_id,
                instrument_id: instrument.id(),
                position_id: prior.id,
                quantity,
                cost: Decimal::ZERO,
                income: Some(collateral),
                settled: true,
                event_id: UUID4::new(),
                ts_event,
                ts_init,
            };
            return Ok(Some((Vec::new(), vec![leg])));
        }

        let cost = release_cost(&prior, quantity)?;
        let position_id = prior.id;
        let leg = build_leg(
            result,
            PositionOperationDirection::Close,
            &instrument,
            position_id,
            quantity,
            cost,
            Some(collateral),
            ts_event,
            ts_init,
        );
        Ok(Some((
            vec![OperationLegPlan {
                instrument,
                prior: Some(prior),
                leg,
            }],
            Vec::new(),
        )))
    }

    fn plan_convert(
        &self,
        result: &PositionOperationResult,
        mut received: Vec<(InstrumentAny, Quantity)>,
        mut spent: Vec<(InstrumentAny, Quantity)>,
        collateral: Decimal,
        ts_event: nautilus_core::UnixNanos,
        ts_init: nautilus_core::UnixNanos,
    ) -> anyhow::Result<Vec<OperationLegPlan>> {
        anyhow::ensure!(!spent.is_empty(), "convert requires NO input tokens");
        anyhow::ensure!(
            !collateral.is_sign_negative(),
            "convert collateral return must be non-negative"
        );
        spent.sort_by_key(|(instrument, _)| instrument.id());
        received.sort_by_key(|(instrument, _)| instrument.id());

        let mut inputs = Vec::with_capacity(spent.len());
        let mut consumed_cost = Decimal::ZERO;
        for (instrument, quantity) in spent {
            let prior = self
                .existing_position(&instrument.id(), result.strategy_id, result.account_id)?
                .ok_or_else(|| {
                    anyhow::anyhow!("convert input position is not open for {}", instrument.id())
                })?;
            let cost = release_cost(&prior, quantity)?;
            consumed_cost += cost;
            inputs.push((instrument, prior, quantity, cost));
        }

        let has_outputs = !received.is_empty();
        let immediate_pnl = if has_outputs {
            (collateral - consumed_cost).max(Decimal::ZERO)
        } else {
            collateral - consumed_cost
        };
        let output_cost = if has_outputs {
            (consumed_cost - collateral).max(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        let pnl_weights: Vec<Decimal> = if consumed_cost.is_zero() {
            inputs
                .iter()
                .map(|(_, _, quantity, _)| quantity.as_decimal())
                .collect()
        } else {
            inputs.iter().map(|(_, _, _, cost)| *cost).collect()
        };
        let pnl_shares = allocate(immediate_pnl, &pnl_weights);

        let mut plans = Vec::with_capacity(inputs.len() + received.len());
        for ((instrument, prior, quantity, cost), pnl_share) in inputs.into_iter().zip(pnl_shares) {
            let position_id = prior.id;
            let leg = build_leg(
                result,
                PositionOperationDirection::Close,
                &instrument,
                position_id,
                quantity,
                cost,
                Some(cost + pnl_share),
                ts_event,
                ts_init,
            );
            plans.push(OperationLegPlan {
                instrument,
                prior: Some(prior),
                leg,
            });
        }

        if has_outputs {
            let output_weights: Vec<Decimal> = received
                .iter()
                .map(|(_, quantity)| quantity.as_decimal())
                .collect();
            let output_costs = allocate(output_cost, &output_weights);
            for ((instrument, quantity), cost) in received.into_iter().zip(output_costs) {
                let position_id = netting_position_id(&instrument.id(), result.strategy_id);
                let prior = self.existing_position(
                    &instrument.id(),
                    result.strategy_id,
                    result.account_id,
                )?;
                let leg = build_leg(
                    result,
                    PositionOperationDirection::Open,
                    &instrument,
                    position_id,
                    quantity,
                    cost,
                    None,
                    ts_event,
                    ts_init,
                );
                plans.push(OperationLegPlan {
                    instrument,
                    prior,
                    leg,
                });
            }
        }

        Ok(plans)
    }

    fn commit_operation_legs(
        &self,
        plans: Vec<OperationLegPlan>,
    ) -> anyhow::Result<Vec<PositionOperationLeg>> {
        let ts_init = self.clock.borrow().timestamp_ns();
        let mut updates = Vec::with_capacity(plans.len());
        let mut events = Vec::with_capacity(plans.len());
        let mut legs = Vec::with_capacity(plans.len());

        for plan in plans {
            let before_flat = plan.prior.as_ref().is_none_or(Position::is_closed);
            let position = match &plan.prior {
                Some(prior) => {
                    let mut position = prior.clone();
                    position
                        .apply_operation_leg(plan.leg.clone())
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    position
                }
                None => {
                    if plan.leg.direction == PositionOperationDirection::Close {
                        anyhow::bail!(
                            "close operation leg has no prior position for {}",
                            plan.instrument.id()
                        );
                    }
                    Position::new_from_operation_checked(&plan.instrument, plan.leg.clone())
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                }
            };

            let event = if before_flat && position.is_open() {
                PositionEvent::PositionOpened(PositionOpened::create_from_operation(
                    &position,
                    &plan.leg,
                    UUID4::new(),
                    ts_init,
                ))
            } else if !before_flat && position.is_closed() {
                PositionEvent::PositionClosed(PositionClosed::create_from_operation(
                    &position,
                    &plan.leg,
                    UUID4::new(),
                    ts_init,
                ))
            } else {
                PositionEvent::PositionChanged(PositionChanged::create_from_operation(
                    &position,
                    &plan.leg,
                    UUID4::new(),
                    ts_init,
                ))
            };

            updates.push((position, plan.prior));
            events.push(event);
            legs.push(plan.leg);
        }

        {
            let cache = self.cache.borrow();
            for (position, _) in &updates {
                cache.validate_position_update(position, OmsType::Netting)?;
            }
        }

        // Archive any prior closed cycles before their NETTING position ID is reused.
        for (_, prior) in &updates {
            if let Some(prior) = prior
                && prior.is_closed()
            {
                self.snapshot_position(prior)?;
            }
        }

        let mut persistence_errors = Vec::new();
        {
            let mut cache = self.cache.borrow_mut();
            for (position, prior) in &updates {
                let result = if prior.is_some() {
                    cache.replace_position(position, OmsType::Netting, false, false)
                } else {
                    cache.add_position_without_order(position, OmsType::Netting)
                };
                if let Err(e) = result {
                    log::error!(
                        "Position operation leg for {} was applied in memory but not persisted: {e}",
                        position.id
                    );
                    persistence_errors.push(e.to_string());
                }
            }
        }

        {
            let cache = self.cache.borrow();
            for (position, _) in &updates {
                anyhow::ensure!(
                    cache
                        .position(&position.id)
                        .is_some_and(|cached| cached.as_ref() == position),
                    "position operation leg for {} was not fully applied",
                    position.id
                );
            }
        }

        if !persistence_errors.is_empty() {
            log::error!(
                "Position operation applied in memory with {} persistence error(s): {}",
                persistence_errors.len(),
                persistence_errors.join("; ")
            );
        }

        self.publish_position_events(events);
        Ok(legs)
    }
}

#[derive(Clone, Debug)]
struct ExpectedOperationLeg {
    position_id: PositionId,
    instrument_id: InstrumentId,
    strategy_id: StrategyId,
    direction: PositionOperationDirection,
    quantity: Quantity,
    cost: Option<Decimal>,
    income_known: bool,
    income: Option<Decimal>,
    settled: bool,
}

fn expected_from_record(
    recovery: &PositionOperationRecovery,
) -> anyhow::Result<Vec<ExpectedOperationLeg>> {
    let mut seen = AHashSet::new();
    let mut legs = Vec::with_capacity(recovery.applied_legs.len());

    for leg in &recovery.applied_legs {
        anyhow::ensure!(
            recovery.instrument_ids.contains(&leg.instrument_id),
            "recorded operation leg instrument {} is outside the barrier scope",
            leg.instrument_id
        );
        anyhow::ensure!(
            leg.quantity.as_decimal() > Decimal::ZERO,
            "recorded operation leg quantity must be positive"
        );
        anyhow::ensure!(
            !leg.cost.is_sign_negative(),
            "recorded operation leg cost must not be negative"
        );
        anyhow::ensure!(
            seen.insert((leg.instrument_id, leg.position_id, leg.direction)),
            "recorded operation legs contain a duplicate identity"
        );
        match leg.direction {
            PositionOperationDirection::Open => anyhow::ensure!(
                leg.income.is_none(),
                "recorded opening operation leg must not carry income"
            ),
            PositionOperationDirection::Close => anyhow::ensure!(
                leg.income.is_some_and(|income| !income.is_sign_negative()),
                "recorded closing operation leg must carry non-negative income"
            ),
        }

        legs.push(ExpectedOperationLeg {
            position_id: leg.position_id,
            instrument_id: leg.instrument_id,
            strategy_id: leg.strategy_id.unwrap_or(recovery.strategy_id),
            direction: leg.direction,
            quantity: leg.quantity,
            cost: Some(leg.cost),
            income_known: true,
            income: leg.income,
            settled: leg.settled,
        });
    }

    match recovery.operation {
        PositionOperationType::Split => {
            anyhow::ensure!(
                legs.len() == 2
                    && legs
                        .iter()
                        .all(|leg| leg.direction == PositionOperationDirection::Open),
                "recorded split requires two opening legs"
            );
        }
        PositionOperationType::Merge => {
            anyhow::ensure!(
                legs.len() == 2
                    && legs
                        .iter()
                        .all(|leg| leg.direction == PositionOperationDirection::Close),
                "recorded merge requires two closing legs"
            );
        }
        PositionOperationType::Redeem => {
            anyhow::ensure!(
                !legs.is_empty()
                    && legs
                        .iter()
                        .all(|leg| leg.direction == PositionOperationDirection::Close),
                "recorded redeem requires closing legs"
            );
        }
        PositionOperationType::Convert => {
            anyhow::ensure!(
                legs.iter()
                    .any(|leg| leg.direction == PositionOperationDirection::Close),
                "recorded convert requires at least one closing leg"
            );
        }
    }

    Ok(legs)
}

fn validate_recovery_leg(
    recovery: &PositionOperationRecovery,
    expected: &ExpectedOperationLeg,
    replayed: &PositionOperationLeg,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        replayed.operation == recovery.operation,
        "operation replay leg type does not match its record"
    );
    anyhow::ensure!(
        replayed.direction == expected.direction
            && replayed.instrument_id == expected.instrument_id
            && replayed.position_id == expected.position_id
            && replayed.strategy_id == expected.strategy_id,
        "operation replay leg identity does not match its record"
    );
    anyhow::ensure!(
        replayed.quantity == expected.quantity,
        "operation replay quantity for {} does not match its record",
        expected.instrument_id
    );
    if let Some(cost) = expected.cost {
        anyhow::ensure!(
            replayed.cost == cost,
            "operation replay cost for {} does not match its record",
            expected.instrument_id
        );
    }
    if expected.income_known {
        anyhow::ensure!(
            replayed.income == expected.income,
            "operation replay income for {} does not match its record",
            expected.instrument_id
        );
    }
    Ok(())
}

fn expected_settled_leg(
    recovery: &PositionOperationRecovery,
    leg: &ExpectedOperationLeg,
    ts: nautilus_core::UnixNanos,
) -> PositionOperationLeg {
    PositionOperationLeg {
        operation_id: recovery.operation_id,
        operation: recovery.operation,
        direction: PositionOperationDirection::Close,
        trader_id: recovery.trader_id,
        strategy_id: leg.strategy_id,
        account_id: recovery.account_id,
        instrument_id: leg.instrument_id,
        position_id: leg.position_id,
        quantity: leg.quantity,
        cost: leg.cost.unwrap_or_default(),
        income: leg.income,
        settled: true,
        event_id: UUID4::new(),
        ts_event: ts,
        ts_init: ts,
    }
}

fn recovery_legs(legs: &[PositionOperationLeg]) -> Vec<PositionOperationRecoveryLeg> {
    legs.iter()
        .map(|leg| PositionOperationRecoveryLeg {
            position_id: leg.position_id,
            instrument_id: leg.instrument_id,
            strategy_id: Some(leg.strategy_id),
            direction: leg.direction,
            quantity: leg.quantity,
            cost: leg.cost,
            income: leg.income,
            settled: leg.settled,
        })
        .collect()
}

const SETTLED_CONSUMPTION_PREFIX: &str = "position-operation-settled";

fn settled_consumption_key(
    position_id: PositionId,
    cycle_opened: nautilus_core::UnixNanos,
) -> String {
    format!("{SETTLED_CONSUMPTION_PREFIX}:{position_id}:{cycle_opened}")
}

fn settled_operation_key(
    operation_id: &UUID4,
    position_id: PositionId,
    cycle_opened: nautilus_core::UnixNanos,
) -> String {
    format!("{SETTLED_CONSUMPTION_PREFIX}:{operation_id}:{position_id}:{cycle_opened}")
}

fn settled_operation_consumed(
    cache: &Cache,
    operation_id: UUID4,
    position_id: PositionId,
    cycle_opened: nautilus_core::UnixNanos,
) -> Decimal {
    cache
        .get(&settled_operation_key(
            &operation_id,
            position_id,
            cycle_opened,
        ))
        .ok()
        .flatten()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|value| Decimal::from_str(value).ok())
        .unwrap_or_default()
}

fn settled_consumed(cache: &Cache, position: &Position) -> Decimal {
    cache
        .get(&settled_consumption_key(position.id, position.ts_opened))
        .ok()
        .flatten()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|value| Decimal::from_str(value).ok())
        .unwrap_or_default()
}

/// Returns the physical quantity held when the position was economically settled.
///
/// This replays the retained cycle and requires an `InstrumentClosed` event with no adjustments
/// or void history that could change the quantity without a replayable transaction.
fn settled_physical_quantity(position: &Position) -> Option<Quantity> {
    if !position.is_closed() || !position.adjustments.is_empty() || !position.fill_voids.is_empty()
    {
        return None;
    }

    let mut quantity = Decimal::ZERO;
    let mut settled = None;
    let mut reopened_after_settlement = false;

    for event in &position.replay_events {
        match event {
            PositionReplayEvent::Filled(fill) => {
                reopened_after_settlement = settled.is_some();
                match fill.order_side {
                    OrderSide::Buy => quantity += fill.last_qty.as_decimal(),
                    OrderSide::Sell => quantity -= fill.last_qty.as_decimal(),
                }
            }
            PositionReplayEvent::Operation(leg) => {
                reopened_after_settlement = settled.is_some();
                match leg.direction {
                    PositionOperationDirection::Open => {
                        quantity += leg.quantity.as_decimal();
                    }
                    PositionOperationDirection::Close => {
                        quantity -= leg.quantity.as_decimal();
                    }
                }
            }
            PositionReplayEvent::InstrumentClosed(_) => {
                if quantity <= Decimal::ZERO {
                    return None;
                }
                settled = Some(quantity);
                quantity = Decimal::ZERO;
                reopened_after_settlement = false;
            }
            PositionReplayEvent::Adjusted(_) => return None,
        }

        if quantity < Decimal::ZERO {
            return None;
        }
    }

    if reopened_after_settlement {
        return None;
    }

    Quantity::from_decimal_dp(settled?, position.size_precision).ok()
}

fn resolve_effects(
    amounts: &[PositionOperationTokenAmount],
    tokens: &AHashMap<ustr::Ustr, InstrumentAny>,
) -> anyhow::Result<Vec<(InstrumentAny, Quantity)>> {
    let mut resolved = Vec::with_capacity(amounts.len());
    for amount in amounts {
        let instrument = tokens
            .get(&ustr::Ustr::from(amount.token_id.as_str()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no cached instrument maps token {} from position operation effects",
                    amount.token_id
                )
            })?
            .clone();
        let quantity = Quantity::from(amount.quantity.as_str());
        resolved.push((instrument, quantity));
    }
    Ok(resolved)
}

fn netting_position_id(instrument_id: &InstrumentId, strategy_id: StrategyId) -> PositionId {
    PositionId::new(format!("{instrument_id}-{strategy_id}"))
}

#[allow(
    clippy::too_many_arguments,
    reason = "leg fields mirror the operation identity and exact allocation"
)]
fn build_leg(
    result: &PositionOperationResult,
    direction: PositionOperationDirection,
    instrument: &InstrumentAny,
    position_id: PositionId,
    quantity: Quantity,
    cost: Decimal,
    income: Option<Decimal>,
    ts_event: nautilus_core::UnixNanos,
    ts_init: nautilus_core::UnixNanos,
) -> PositionOperationLeg {
    PositionOperationLeg {
        operation_id: result.operation_id,
        operation: result.operation,
        direction,
        trader_id: result.trader_id,
        strategy_id: result.strategy_id,
        account_id: result.account_id,
        instrument_id: instrument.id(),
        position_id,
        quantity,
        cost,
        income,
        settled: false,
        event_id: UUID4::new(),
        ts_event,
        ts_init,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "leg fields mirror the operation identity and exact allocation"
)]
fn build_position_leg(
    result: &PositionOperationResult,
    direction: PositionOperationDirection,
    instrument: &InstrumentAny,
    position: &Position,
    quantity: Quantity,
    cost: Decimal,
    income: Option<Decimal>,
    ts_event: nautilus_core::UnixNanos,
    ts_init: nautilus_core::UnixNanos,
) -> PositionOperationLeg {
    PositionOperationLeg {
        operation_id: result.operation_id,
        operation: result.operation,
        direction,
        trader_id: position.trader_id,
        strategy_id: position.strategy_id,
        account_id: position.account_id,
        instrument_id: instrument.id(),
        position_id: position.id,
        quantity,
        cost,
        income,
        settled: false,
        event_id: UUID4::new(),
        ts_event,
        ts_init,
    }
}

fn release_cost(position: &Position, quantity: Quantity) -> anyhow::Result<Decimal> {
    anyhow::ensure!(
        position.is_long(),
        "operation input position is not long for {}",
        position.id
    );
    let quantity = quantity.as_decimal();
    let prior = position.signed_decimal_qty();
    anyhow::ensure!(
        !quantity.is_zero() && quantity <= prior,
        "operation quantity exceeds open position quantity for {}",
        position.id
    );

    if quantity == prior {
        Ok(position.cost_open)
    } else {
        let cost = (position.cost_open * quantity)
            .checked_div(prior)
            .ok_or_else(|| anyhow::anyhow!("position cost allocation division failed"))?;
        anyhow::ensure!(
            cost <= position.cost_open,
            "position cost allocation exceeds open cost"
        );
        Ok(cost)
    }
}

fn allocate(total: Decimal, weights: &[Decimal]) -> Vec<Decimal> {
    debug_assert!(!weights.is_empty());
    let sum: Decimal = weights.iter().copied().sum();
    let count = Decimal::from(weights.len());
    let mut allocated = Decimal::ZERO;
    let mut shares = Vec::with_capacity(weights.len());

    for (index, weight) in weights.iter().enumerate() {
        if index + 1 == weights.len() {
            shares.push(total - allocated);
            break;
        }

        let share = if sum.is_zero() {
            total / count
        } else {
            total * *weight / sum
        };
        allocated += share;
        shares.push(share);
    }

    shares
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::VirtualClock,
        messages::execution::PositionOperationEffects,
        msgbus::{self, switchboard, typed_handler::TypedHandler},
    };
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::{
        data::InstrumentClose,
        enums::{AssetClass, InstrumentCloseType, LiquiditySide, OrderSide, OrderType},
        events::OrderEventAny,
        identifiers::{
            AccountId, ClientId, InstrumentId, StrategyId, Symbol, TradeId, TraderId, VenueOrderId,
        },
        instruments::{BinaryOption, InstrumentAny},
        orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
        position_operation::{PositionOperationDirection, PositionOperationLeg},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn option(token_id: &str, outcome: &str) -> InstrumentAny {
        let info = Params::new(
            [
                (
                    "token_id".to_string(),
                    serde_json::Value::String(token_id.to_string()),
                ),
                (
                    "condition_id".to_string(),
                    serde_json::Value::String("0xcondition".to_string()),
                ),
            ]
            .into(),
        );
        InstrumentAny::BinaryOption(
            BinaryOption::builder()
                .instrument_id(InstrumentId::from(format!("{token_id}.POLYMARKET")))
                .raw_symbol(Symbol::new(token_id))
                .asset_class(AssetClass::Alternative)
                .currency(Currency::pUSD())
                .activation_ns(UnixNanos::default())
                .expiration_ns(UnixNanos::from(1))
                .price_precision(2)
                .size_precision(6)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.000001"))
                .outcome(outcome.into())
                .info(info)
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn engine_with(instruments: &[InstrumentAny]) -> (ExecutionEngine, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        for instrument in instruments {
            cache
                .borrow_mut()
                .add_instrument(instrument.clone())
                .unwrap();
        }
        let engine = ExecutionEngine::new(
            Rc::new(RefCell::new(VirtualClock::new())),
            cache.clone(),
            None,
        );
        (engine, cache)
    }

    fn result(
        operation: PositionOperationType,
        instrument_ids: Vec<InstrumentId>,
        effects: PositionOperationEffects,
    ) -> PositionOperationResult {
        PositionOperationResult::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRATEGY-001"),
            ClientId::from("POLYMARKET"),
            AccountId::from("ACCOUNT-001"),
            UUID4::new(),
            operation,
            PositionOperationStatus::Confirmed,
            instrument_ids,
            None,
            None,
            None,
            Some(effects),
            UUID4::new(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_001),
        )
    }

    fn amount(token_id: &str, quantity: &str) -> PositionOperationTokenAmount {
        PositionOperationTokenAmount {
            token_id: token_id.into(),
            quantity: quantity.into(),
        }
    }

    fn short_position(
        instrument: &InstrumentAny,
        strategy_id: StrategyId,
        account_id: AccountId,
        quantity: Quantity,
    ) -> Position {
        let position_id = netting_position_id(&instrument.id(), strategy_id);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(quantity)
            .build();
        let event = TestOrderEventStubs::filled(
            &order,
            instrument,
            Some(TradeId::new("SHORT-1")),
            Some(position_id),
            Some(Price::from("0.50")),
            Some(quantity),
            Some(LiquiditySide::Taker),
            None,
            None,
            Some(account_id),
        );
        let OrderEventAny::Filled(fill) = event else {
            panic!("expected a filled event");
        };
        Position::new(instrument, fill)
    }

    fn input_position(
        instrument: &InstrumentAny,
        strategy_id: StrategyId,
        account_id: AccountId,
        quantity: Quantity,
        cost: Decimal,
    ) -> Position {
        let position_id = netting_position_id(&instrument.id(), strategy_id);
        let leg = PositionOperationLeg {
            operation_id: UUID4::new(),
            operation: PositionOperationType::Convert,
            direction: PositionOperationDirection::Open,
            trader_id: TraderId::from("TRADER-001"),
            strategy_id,
            account_id,
            instrument_id: instrument.id(),
            position_id,
            quantity,
            cost,
            income: None,
            settled: false,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(2),
        };
        Position::new_from_operation_checked(instrument, leg).unwrap()
    }

    #[rstest]
    fn test_split_application_allocates_cost_and_is_idempotent() {
        let yes = option("100", "Yes");
        let no = option("101", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let result = result(
            PositionOperationType::Split,
            vec![yes.id(), no.id()],
            PositionOperationEffects {
                received: vec![amount("100", "100"), amount("101", "100")],
                spent: Vec::new(),
                collateral_delta: "-50".into(),
                payouts: Vec::new(),
            },
        );

        engine.process_position_operation(&result);

        let strategy = StrategyId::from("STRATEGY-001");
        let yes_position = cache
            .borrow()
            .position(&netting_position_id(&yes.id(), strategy))
            .unwrap()
            .cloned();
        let no_position = cache
            .borrow()
            .position(&netting_position_id(&no.id(), strategy))
            .unwrap()
            .cloned();
        assert_eq!(yes_position.quantity, Quantity::from("100"));
        assert_eq!(yes_position.cost_open, dec!(25));
        assert_eq!(yes_position.avg_px_open, dec!(0.25));
        assert_eq!(no_position.cost_open, dec!(25));
        assert_eq!(no_position.opening_operation_id, Some(result.operation_id));

        engine.process_position_operation(&result);

        let yes_position = cache
            .borrow()
            .position(&netting_position_id(&yes.id(), strategy))
            .unwrap()
            .cloned();
        assert_eq!(yes_position.quantity, Quantity::from("100"));
        assert_eq!(yes_position.cost_open, dec!(25));
    }

    #[rstest]
    fn test_convert_application_rejects_short_input() {
        let no = option("775", "No");
        let yes = option("776", "Yes");
        let (mut engine, cache) = engine_with(&[no.clone(), yes.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = short_position(&no, strategy, account, Quantity::from("100"));
        let position_id = position.id;
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let result = result(
            PositionOperationType::Convert,
            vec![no.id(), yes.id()],
            PositionOperationEffects {
                received: vec![amount("776", "100")],
                spent: vec![amount("775", "100")],
                collateral_delta: "0".into(),
                payouts: Vec::new(),
            },
        );
        engine.process_position_operation(&result);

        let cache = cache.borrow();
        let position = cache
            .position(&position_id)
            .expect("short position must remain");
        assert!(position.is_short());
        assert_eq!(position.quantity, Quantity::from("100"));
        assert!(
            cache
                .position(&netting_position_id(&yes.id(), strategy))
                .is_none()
        );
    }

    #[rstest]
    fn test_convert_application_transfers_cost_without_immediate_pnl() {
        let no_a = option("200", "No");
        let no_b = option("201", "No");
        let yes_c = option("300", "Yes");
        let (mut engine, cache) = engine_with(&[no_a.clone(), no_b.clone(), yes_c.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&no_a, dec!(80)), (&no_b, dec!(70))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let result = result(
            PositionOperationType::Convert,
            vec![no_a.id(), no_b.id(), yes_c.id()],
            PositionOperationEffects {
                received: vec![amount("300", "100")],
                spent: vec![amount("200", "100"), amount("201", "100")],
                collateral_delta: "100".into(),
                payouts: Vec::new(),
            },
        );

        engine.process_position_operation(&result);

        let no_a_position = cache
            .borrow()
            .position(&netting_position_id(&no_a.id(), strategy))
            .unwrap()
            .cloned();
        let no_b_position = cache
            .borrow()
            .position(&netting_position_id(&no_b.id(), strategy))
            .unwrap()
            .cloned();
        let yes_position = cache
            .borrow()
            .position(&netting_position_id(&yes_c.id(), strategy))
            .unwrap()
            .cloned();

        assert!(no_a_position.is_closed());
        assert!(no_b_position.is_closed());
        assert_eq!(
            no_a_position.realized_pnl,
            Some(nautilus_model::types::Money::zero(Currency::pUSD()))
        );
        assert_eq!(
            no_b_position.realized_pnl,
            Some(nautilus_model::types::Money::zero(Currency::pUSD()))
        );
        assert_eq!(yes_position.quantity, Quantity::from("100"));
        assert_eq!(yes_position.cost_open, dec!(50));
    }

    #[rstest]
    fn test_recovery_repairs_already_applied_convert_without_events() {
        let no_a = option("210", "No");
        let no_b = option("211", "No");
        let yes_c = option("310", "Yes");
        let (mut engine, cache) = engine_with(&[no_a.clone(), no_b.clone(), yes_c.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&no_a, dec!(80)), (&no_b, dec!(70))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let effects = PositionOperationEffects {
            received: vec![amount("310", "100")],
            spent: vec![amount("210", "100"), amount("211", "100")],
            collateral_delta: "100".into(),
            payouts: Vec::new(),
        };
        let published = Rc::new(RefCell::new(Vec::<PositionEvent>::new()));
        let handler = TypedHandler::from({
            let published = published.clone();
            move |event: &PositionEvent| published.borrow_mut().push(event.clone())
        });
        msgbus::subscribe_position_events(
            switchboard::get_event_position_topic(strategy).into(),
            handler,
            None,
        );

        let result = result(
            PositionOperationType::Convert,
            vec![no_a.id(), no_b.id(), yes_c.id()],
            effects.clone(),
        );
        engine.process_position_operation(&result);

        let published_count = published.borrow().len();
        assert!(
            published_count > 0,
            "initial application must publish position events"
        );

        let before = cache
            .borrow()
            .positions(None, None, None, None, None)
            .into_iter()
            .map(|position| position.cloned())
            .collect::<Vec<_>>();
        let replay_count = replay_legs(&cache, result.operation_id).len();
        assert_eq!(replay_count, 3);

        let recovery = recovery_request(&result, effects, Vec::new());
        let outcome = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();

        let PositionOperationRecoveryOutcome::Applied { legs } = outcome else {
            panic!("expected applied recovery");
        };
        assert_eq!(legs.len(), 3);

        let after = cache
            .borrow()
            .positions(None, None, None, None, None)
            .into_iter()
            .map(|position| position.cloned())
            .collect::<Vec<_>>();
        assert_eq!(before, after);
        assert_eq!(replay_legs(&cache, result.operation_id).len(), replay_count);
        assert_eq!(
            published.borrow().len(),
            published_count,
            "recovery must not republish position events"
        );

        let repeated = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();
        assert!(matches!(
            repeated,
            PositionOperationRecoveryOutcome::Applied { .. }
        ));
        assert_eq!(
            published.borrow().len(),
            published_count,
            "repeated recovery must not republish position events"
        );
    }

    #[rstest]
    fn test_recovery_completes_partial_convert_from_recorded_legs() {
        let no_a = option("220", "No");
        let no_b = option("221", "No");
        let yes_c = option("320", "Yes");
        let (mut engine, cache) = engine_with(&[no_a.clone(), no_b.clone(), yes_c.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&no_a, dec!(80)), (&no_b, dec!(70))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let effects = PositionOperationEffects {
            received: vec![amount("320", "100")],
            spent: vec![amount("220", "100"), amount("221", "100")],
            collateral_delta: "100".into(),
            payouts: Vec::new(),
        };
        let result = result(
            PositionOperationType::Convert,
            vec![no_a.id(), no_b.id(), yes_c.id()],
            effects.clone(),
        );

        // Simulate a crash after the input legs were persisted but before the output leg existed.
        let mut recorded = Vec::new();
        for (instrument, cost) in [(&no_a, dec!(80)), (&no_b, dec!(70))] {
            let position_id = netting_position_id(&instrument.id(), strategy);
            let leg = PositionOperationLeg {
                operation_id: result.operation_id,
                operation: PositionOperationType::Convert,
                direction: PositionOperationDirection::Close,
                trader_id: result.trader_id,
                strategy_id: strategy,
                account_id: account,
                instrument_id: instrument.id(),
                position_id,
                quantity: Quantity::from("100"),
                cost,
                income: Some(cost),
                settled: false,
                event_id: UUID4::new(),
                ts_event: UnixNanos::from(1_000_000_000),
                ts_init: UnixNanos::from(1_000_000_001),
            };
            let mut position = cache.borrow().position(&position_id).unwrap().cloned();
            position.apply_operation_leg(leg.clone()).unwrap();
            cache
                .borrow_mut()
                .replace_position(&position, OmsType::Netting, false, false)
                .unwrap();
            recorded.push(PositionOperationRecoveryLeg {
                position_id,
                instrument_id: instrument.id(),
                strategy_id: Some(strategy),
                direction: leg.direction,
                quantity: leg.quantity,
                cost: leg.cost,
                income: leg.income,
                settled: leg.settled,
            });
        }
        let yes_position_id = netting_position_id(&yes_c.id(), strategy);
        recorded.push(PositionOperationRecoveryLeg {
            position_id: yes_position_id,
            instrument_id: yes_c.id(),
            strategy_id: Some(strategy),
            direction: PositionOperationDirection::Open,
            quantity: Quantity::from("100"),
            cost: dec!(50),
            income: None,
            settled: false,
        });

        assert_eq!(replay_legs(&cache, result.operation_id).len(), 2);
        assert!(cache.borrow().position(&yes_position_id).is_none());

        let recovery = recovery_request(&result, effects, recorded);
        let outcome = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();

        let PositionOperationRecoveryOutcome::Applied { legs } = outcome else {
            panic!("expected completed recovery");
        };
        assert_eq!(legs.len(), 3);
        assert_eq!(replay_legs(&cache, result.operation_id).len(), 3);

        let restored_yes = cache.borrow().position(&yes_position_id).unwrap().cloned();
        assert_eq!(restored_yes.quantity, Quantity::from("100"));
        assert_eq!(restored_yes.cost_open, dec!(50));
    }

    #[rstest]
    fn test_recovery_rejects_conflicting_recorded_cost() {
        let no_a = option("230", "No");
        let no_b = option("231", "No");
        let yes_c = option("330", "Yes");
        let (mut engine, cache) = engine_with(&[no_a.clone(), no_b.clone(), yes_c.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&no_a, dec!(80)), (&no_b, dec!(70))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let effects = PositionOperationEffects {
            received: vec![amount("330", "100")],
            spent: vec![amount("230", "100"), amount("231", "100")],
            collateral_delta: "100".into(),
            payouts: Vec::new(),
        };
        let result = result(
            PositionOperationType::Convert,
            vec![no_a.id(), no_b.id(), yes_c.id()],
            effects.clone(),
        );
        engine.process_position_operation(&result);

        let mut recorded = replay_legs(&cache, result.operation_id)
            .into_iter()
            .map(|leg| PositionOperationRecoveryLeg {
                position_id: leg.position_id,
                instrument_id: leg.instrument_id,
                strategy_id: Some(leg.strategy_id),
                direction: leg.direction,
                quantity: leg.quantity,
                cost: leg.cost,
                income: leg.income,
                settled: leg.settled,
            })
            .collect::<Vec<_>>();
        recorded[0].cost += dec!(1);

        let recovery = recovery_request(&result, effects, recorded);
        assert!(
            engine
                .reconcile_recovered_position_operation(&recovery)
                .is_err()
        );
        assert_eq!(replay_legs(&cache, result.operation_id).len(), 3);
    }

    #[rstest]
    fn test_recovery_applies_unapplied_convert_once() {
        let no_a = option("240", "No");
        let no_b = option("241", "No");
        let yes_c = option("340", "Yes");
        let (mut engine, cache) = engine_with(&[no_a.clone(), no_b.clone(), yes_c.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&no_a, dec!(80)), (&no_b, dec!(70))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let effects = PositionOperationEffects {
            received: vec![amount("340", "100")],
            spent: vec![amount("240", "100"), amount("241", "100")],
            collateral_delta: "100".into(),
            payouts: Vec::new(),
        };
        let result = result(
            PositionOperationType::Convert,
            vec![no_a.id(), no_b.id(), yes_c.id()],
            effects.clone(),
        );
        let recovery = recovery_request(&result, effects, Vec::new());

        let outcome = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();
        let PositionOperationRecoveryOutcome::Applied { legs } = outcome else {
            panic!("expected applied recovery");
        };
        assert_eq!(legs.len(), 3);
        assert_eq!(replay_legs(&cache, result.operation_id).len(), 3);

        let yes_position = cache
            .borrow()
            .position(&netting_position_id(&yes_c.id(), strategy))
            .unwrap()
            .cloned();
        assert_eq!(yes_position.quantity, Quantity::from("100"));
        assert_eq!(yes_position.cost_open, dec!(50));

        let repeated = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();
        assert!(matches!(
            repeated,
            PositionOperationRecoveryOutcome::Applied { .. }
        ));
        assert_eq!(replay_legs(&cache, result.operation_id).len(), 3);
        let repeated_yes = cache
            .borrow()
            .position(&netting_position_id(&yes_c.id(), strategy))
            .unwrap()
            .cloned();
        assert_eq!(repeated_yes, yes_position);
    }

    fn recovery_request(
        result: &PositionOperationResult,
        effects: PositionOperationEffects,
        applied_legs: Vec<PositionOperationRecoveryLeg>,
    ) -> PositionOperationRecovery {
        PositionOperationRecovery {
            trader_id: result.trader_id,
            strategy_id: result.strategy_id,
            client_id: result.client_id,
            account_id: result.account_id,
            operation_id: result.operation_id,
            operation: result.operation,
            instrument_ids: result.instrument_ids.clone(),
            quantity: None,
            effects: Some(effects),
            venue_transaction_id: None,
            transaction_hash: None,
            applied_legs,
        }
    }

    fn replay_legs(cache: &Rc<RefCell<Cache>>, operation_id: UUID4) -> Vec<PositionOperationLeg> {
        cache
            .borrow()
            .positions(None, None, None, None, None)
            .into_iter()
            .flat_map(|position| position.replay_events.clone())
            .filter_map(|event| match event {
                PositionReplayEvent::Operation(leg) if leg.operation_id == operation_id => {
                    Some(leg)
                }
                _ => None,
            })
            .collect()
    }

    #[rstest]
    fn test_barrier_does_not_install_partially_on_conflict() {
        let yes = option("700", "Yes");
        let no = option("701", "No");
        let (engine, _cache) = engine_with(&[yes.clone(), no.clone()]);
        let existing = UUID4::new();

        engine
            .establish_full_barrier(existing, PositionOperationType::Split, &[yes.id()], None)
            .unwrap();
        assert!(
            engine
                .establish_full_barrier(
                    UUID4::new(),
                    PositionOperationType::Split,
                    &[yes.id(), no.id()],
                    None,
                )
                .is_err()
        );
        assert_eq!(engine.position_operation_barrier(&yes.id()), Some(existing));
        assert_eq!(engine.position_operation_barrier(&no.id()), None);
    }

    #[rstest]
    fn test_barrier_rejects_active_clob_orders() {
        let yes = option("710", "Yes");
        let no = option("711", "No");
        let (engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(yes.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("0.50"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let account_id = AccountId::from("ACCOUNT-001");
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        cache.borrow_mut().update_order(&submitted).unwrap();
        let accepted =
            TestOrderEventStubs::accepted(&order, account_id, VenueOrderId::from("V-BARRIER"));
        cache.borrow_mut().update_order(&accepted).unwrap();

        assert!(
            engine
                .establish_full_barrier(
                    UUID4::new(),
                    PositionOperationType::Split,
                    &[yes.id(), no.id()],
                    None,
                )
                .is_err()
        );
        assert_eq!(engine.position_operation_barrier(&yes.id()), None);
        assert_eq!(engine.position_operation_barrier(&no.id()), None);
    }

    #[rstest]
    fn test_merge_application_releases_cost_and_allocates_income() {
        let yes = option("600", "Yes");
        let no = option("601", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&yes, dec!(50)), (&no, dec!(60))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let result = result(
            PositionOperationType::Merge,
            vec![yes.id(), no.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("600", "100"), amount("601", "100")],
                collateral_delta: "120".into(),
                payouts: Vec::new(),
            },
        );
        engine.process_position_operation(&result);

        let yes_position = cache
            .borrow()
            .position(&netting_position_id(&yes.id(), strategy))
            .unwrap()
            .cloned();
        let no_position = cache
            .borrow()
            .position(&netting_position_id(&no.id(), strategy))
            .unwrap()
            .cloned();

        assert!(yes_position.is_closed());
        assert!(no_position.is_closed());
        assert_eq!(
            yes_position.realized_pnl,
            Some(nautilus_model::types::Money::new(10.0, Currency::pUSD()))
        );
        assert_eq!(
            no_position.realized_pnl,
            Some(nautilus_model::types::Money::zero(Currency::pUSD()))
        );
        assert_eq!(replay_legs(&cache, result.operation_id).len(), 2);
    }

    #[rstest]
    fn test_convert_application_carries_loss_when_no_outputs() {
        let no_a = option("400", "No");
        let (mut engine, cache) = engine_with(std::slice::from_ref(&no_a));
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&no_a, strategy, account, Quantity::from("100"), dec!(150));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let result = result(
            PositionOperationType::Convert,
            vec![no_a.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("400", "100")],
                collateral_delta: "100".into(),
                payouts: Vec::new(),
            },
        );

        engine.process_position_operation(&result);

        let position = cache
            .borrow()
            .position(&netting_position_id(&no_a.id(), strategy))
            .unwrap()
            .cloned();
        assert!(position.is_closed());
        assert_eq!(
            position.realized_pnl,
            Some(nautilus_model::types::Money::new(-50.0, Currency::pUSD()))
        );
    }

    #[rstest]
    fn test_ctf_redeem_allocates_across_strategy_owners() {
        let yes = option("520", "Yes");
        let no = option("521", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let account = AccountId::from("ACCOUNT-001");
        let requester = StrategyId::from("STRATEGY-A");
        let second = StrategyId::from("STRATEGY-B");
        let third = StrategyId::from("STRATEGY-C");

        for (instrument, strategy, quantity, cost) in [
            (&yes, requester, "40", dec!(20)),
            (&yes, second, "60", dec!(30)),
            (&no, third, "100", dec!(60)),
        ] {
            let position = input_position(
                instrument,
                strategy,
                account,
                Quantity::from(quantity),
                cost,
            );
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let effects = PositionOperationEffects {
            received: Vec::new(),
            spent: vec![amount("520", "100"), amount("521", "100")],
            collateral_delta: "100".into(),
            payouts: vec![
                PositionOperationTokenPayout {
                    token_id: "520".into(),
                    numerator: "1".into(),
                    denominator: "1".into(),
                },
                PositionOperationTokenPayout {
                    token_id: "521".into(),
                    numerator: "0".into(),
                    denominator: "1".into(),
                },
            ],
        };
        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id(), no.id()],
            effects.clone(),
        );
        engine.process_position_operation(&result);

        let realized = |strategy: StrategyId, instrument: &InstrumentAny| {
            cache
                .borrow()
                .position(&netting_position_id(&instrument.id(), strategy))
                .unwrap()
                .cloned()
        };

        let first_position = realized(requester, &yes);
        let second_position = realized(second, &yes);
        let third_position = realized(third, &no);
        assert!(first_position.is_closed());
        assert!(second_position.is_closed());
        assert!(third_position.is_closed());
        assert_eq!(
            first_position.realized_pnl,
            Some(nautilus_model::types::Money::new(20.0, Currency::pUSD()))
        );
        assert_eq!(
            second_position.realized_pnl,
            Some(nautilus_model::types::Money::new(30.0, Currency::pUSD()))
        );
        assert_eq!(
            third_position.realized_pnl,
            Some(nautilus_model::types::Money::new(-60.0, Currency::pUSD()))
        );

        let legs = replay_legs(&cache, result.operation_id);
        assert_eq!(legs.len(), 3);
        assert_eq!(
            legs.iter().filter(|leg| leg.strategy_id == second).count(),
            1
        );
        assert_eq!(
            legs.iter().filter(|leg| leg.strategy_id == third).count(),
            1
        );

        let recovery = recovery_request(&result, effects, Vec::new());
        let outcome = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();
        assert!(matches!(
            outcome,
            PositionOperationRecoveryOutcome::Applied { .. }
        ));
        assert_eq!(replay_legs(&cache, result.operation_id).len(), 3);
    }

    #[rstest]
    fn test_instrument_close_after_redeem_does_not_settle_again() {
        let yes = option("540", "Yes");
        let no = option("541", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&yes, dec!(40)), (&no, dec!(60))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id(), no.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("540", "100"), amount("541", "100")],
                collateral_delta: "100".into(),
                payouts: vec![
                    PositionOperationTokenPayout {
                        token_id: "540".into(),
                        numerator: "1".into(),
                        denominator: "1".into(),
                    },
                    PositionOperationTokenPayout {
                        token_id: "541".into(),
                        numerator: "0".into(),
                        denominator: "1".into(),
                    },
                ],
            },
        );
        engine.process_position_operation(&result);

        let yes_position_id = netting_position_id(&yes.id(), strategy);
        let before = cache.borrow().position(&yes_position_id).unwrap().cloned();

        engine.settle_instrument_close(&InstrumentClose::new(
            yes.id(),
            Price::from("1"),
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(10),
            UnixNanos::from(10),
        ));

        let after = cache.borrow().position(&yes_position_id).unwrap().cloned();
        assert_eq!(before, after);
    }

    #[rstest]
    fn test_ctf_redeem_records_settled_physical_payout_without_new_pnl() {
        let yes = option("530", "Yes");
        let no = option("531", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost, price) in [(&yes, dec!(40), "1"), (&no, dec!(60), "0")] {
            let mut position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            let close = InstrumentClose::new(
                instrument.id(),
                Price::from(price),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(3),
                UnixNanos::from(3),
            );
            position.apply_instrument_close(close).unwrap();
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let yes_position_id = netting_position_id(&yes.id(), strategy);
        let no_position_id = netting_position_id(&no.id(), strategy);
        let before_yes = cache.borrow().position(&yes_position_id).unwrap().cloned();
        let before_no = cache.borrow().position(&no_position_id).unwrap().cloned();

        let effects = PositionOperationEffects {
            received: Vec::new(),
            spent: vec![amount("530", "100"), amount("531", "100")],
            collateral_delta: "100".into(),
            payouts: vec![
                PositionOperationTokenPayout {
                    token_id: "530".into(),
                    numerator: "1".into(),
                    denominator: "1".into(),
                },
                PositionOperationTokenPayout {
                    token_id: "531".into(),
                    numerator: "0".into(),
                    denominator: "1".into(),
                },
            ],
        };
        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id(), no.id()],
            effects.clone(),
        );
        engine.process_position_operation(&result);

        let after_yes = cache.borrow().position(&yes_position_id).unwrap().cloned();
        let after_no = cache.borrow().position(&no_position_id).unwrap().cloned();
        assert_eq!(before_yes, after_yes);
        assert_eq!(before_no, after_no);
        assert!(replay_legs(&cache, result.operation_id).is_empty());

        let consumed = {
            let cache = cache.borrow();
            settled_consumed(&cache, &cache.position(&yes_position_id).unwrap())
        };
        assert_eq!(consumed, dec!(100));

        let settled_entries = cache.borrow().general_entries_with_prefix(&format!(
            "{SETTLED_CONSUMPTION_PREFIX}:{}:",
            result.operation_id
        ));
        assert!(
            !settled_entries.is_empty(),
            "missing settled consumption entries"
        );

        let recovery = recovery_request(&result, effects.clone(), Vec::new());
        let outcome = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();
        let PositionOperationRecoveryOutcome::Applied { legs } = outcome else {
            panic!("expected applied settled recovery: {outcome:?}");
        };
        assert_eq!(legs.len(), 2);
        assert!(legs.iter().all(|leg| leg.settled));
        assert!(replay_legs(&cache, result.operation_id).is_empty());
        let recovered_yes = cache.borrow().position(&yes_position_id).unwrap().cloned();
        assert_eq!(recovered_yes, before_yes);

        let record_legs = vec![
            PositionOperationRecoveryLeg {
                position_id: yes_position_id,
                instrument_id: yes.id(),
                strategy_id: Some(strategy),
                direction: PositionOperationDirection::Close,
                quantity: Quantity::from("100"),
                cost: Decimal::ZERO,
                income: Some(dec!(100)),
                settled: true,
            },
            PositionOperationRecoveryLeg {
                position_id: no_position_id,
                instrument_id: no.id(),
                strategy_id: Some(strategy),
                direction: PositionOperationDirection::Close,
                quantity: Quantity::from("100"),
                cost: Decimal::ZERO,
                income: Some(dec!(0)),
                settled: true,
            },
        ];
        let recovery = recovery_request(&result, effects, record_legs);
        let outcome = engine
            .reconcile_recovered_position_operation(&recovery)
            .unwrap();
        assert!(matches!(
            outcome,
            PositionOperationRecoveryOutcome::Applied { legs } if legs.iter().all(|leg| leg.settled)
        ));
    }

    #[rstest]
    fn test_settled_consumption_is_idempotent_per_operation_leg() {
        let yes = option("532", "Yes");
        let (engine, cache) = engine_with(std::slice::from_ref(&yes));
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let mut position = input_position(&yes, strategy, account, Quantity::from("100"), dec!(40));
        position
            .apply_instrument_close(InstrumentClose::new(
                yes.id(),
                Price::from("1"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(3),
                UnixNanos::from(3),
            ))
            .unwrap();
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();
        let position_id = position.id;
        let cycle_opened = position.ts_opened;
        let operation_id = UUID4::new();
        let leg = PositionOperationLeg {
            operation_id,
            operation: PositionOperationType::Redeem,
            direction: PositionOperationDirection::Close,
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: strategy,
            account_id: account,
            instrument_id: yes.id(),
            position_id,
            quantity: Quantity::from("40"),
            cost: Decimal::ZERO,
            income: Some(dec!(40)),
            settled: true,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(4),
            ts_init: UnixNanos::from(4),
        };

        engine
            .record_settled_consumption(std::slice::from_ref(&leg))
            .unwrap();
        engine.record_settled_consumption(&[leg]).unwrap();

        let cache = cache.borrow();
        let position = cache.position(&position_id).unwrap();
        assert_eq!(settled_consumed(&cache, &position), dec!(40));
        let operation_consumed = cache
            .get(&settled_operation_key(
                &operation_id,
                position_id,
                cycle_opened,
            ))
            .unwrap()
            .and_then(|bytes| std::str::from_utf8(bytes).ok().map(str::to_string))
            .and_then(|value| Decimal::from_str(&value).ok())
            .unwrap();
        assert_eq!(
            operation_consumed,
            dec!(40),
            "retry must not add the same operation payout twice"
        );
    }

    #[rstest]
    fn test_ctf_redeem_fractional_payout_allocates_exact_remainder() {
        let yes = option("550", "Yes");
        let no = option("551", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let account = AccountId::from("ACCOUNT-001");
        let strategies = [
            StrategyId::from("STRATEGY-A"),
            StrategyId::from("STRATEGY-B"),
            StrategyId::from("STRATEGY-C"),
        ];

        for (strategy, quantity) in strategies.iter().zip(["10", "20", "30"]) {
            let position =
                input_position(&yes, *strategy, account, Quantity::from(quantity), dec!(0));
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }
        let no_position =
            input_position(&no, strategies[0], account, Quantity::from("60"), dec!(0));
        cache
            .borrow_mut()
            .add_position_without_order(&no_position, OmsType::Netting)
            .unwrap();

        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id(), no.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("550", "60"), amount("551", "60")],
                collateral_delta: "60".into(),
                payouts: vec![
                    PositionOperationTokenPayout {
                        token_id: "550".into(),
                        numerator: "1".into(),
                        denominator: "3".into(),
                    },
                    PositionOperationTokenPayout {
                        token_id: "551".into(),
                        numerator: "2".into(),
                        denominator: "3".into(),
                    },
                ],
            },
        );
        engine.process_position_operation(&result);

        let legs = replay_legs(&cache, result.operation_id);
        let yes_incomes: Vec<Decimal> = legs
            .iter()
            .filter(|leg| leg.instrument_id == yes.id())
            .map(|leg| leg.income.unwrap())
            .collect();
        assert_eq!(yes_incomes.len(), 3);
        assert_eq!(yes_incomes.iter().sum::<Decimal>(), dec!(20));
        assert_eq!(yes_incomes[0], dec!(20) * dec!(10) / dec!(60));
    }

    #[rstest]
    fn test_ctf_redeem_single_unit_payout_floors_to_base_units() {
        let yes = option("560", "Yes");
        let (mut engine, cache) = engine_with(std::slice::from_ref(&yes));
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&yes, strategy, account, Quantity::from("1"), dec!(0));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("560", "1")],
                collateral_delta: "0.333333".into(),
                payouts: vec![PositionOperationTokenPayout {
                    token_id: "560".into(),
                    numerator: "1".into(),
                    denominator: "3".into(),
                }],
            },
        );
        engine.process_position_operation(&result);

        let redeemed = cache
            .borrow()
            .position(&netting_position_id(&yes.id(), strategy))
            .unwrap()
            .cloned();
        assert_eq!(
            redeemed.realized_pnl,
            Some(
                nautilus_model::types::Money::from_decimal(dec!(0.333333), Currency::pUSD())
                    .unwrap()
            )
        );
    }

    #[rstest]
    fn test_v2_redeem_applies_single_outcome_payout() {
        let yes = option("510", "Yes");
        let (mut engine, cache) = engine_with(std::slice::from_ref(&yes));
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&yes, strategy, account, Quantity::from("100"), dec!(60));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("510", "100")],
                collateral_delta: "100".into(),
                payouts: Vec::new(),
            },
        );
        engine.process_position_operation(&result);

        let redeemed = cache
            .borrow()
            .position(&netting_position_id(&yes.id(), strategy))
            .unwrap()
            .cloned();
        assert!(redeemed.is_closed());
        assert_eq!(
            redeemed.realized_pnl,
            Some(nautilus_model::types::Money::new(40.0, Currency::pUSD()))
        );
    }

    #[rstest]
    fn test_ctf_redeem_allocates_payout_by_outcome() {
        let yes = option("500", "Yes");
        let no = option("501", "No");
        let (mut engine, cache) = engine_with(&[yes.clone(), no.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");

        for (instrument, cost) in [(&yes, dec!(40)), (&no, dec!(60))] {
            let position =
                input_position(instrument, strategy, account, Quantity::from("100"), cost);
            cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id(), no.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("500", "100"), amount("501", "100")],
                collateral_delta: "100".into(),
                payouts: vec![
                    PositionOperationTokenPayout {
                        token_id: "500".into(),
                        numerator: "1".into(),
                        denominator: "1".into(),
                    },
                    PositionOperationTokenPayout {
                        token_id: "501".into(),
                        numerator: "0".into(),
                        denominator: "1".into(),
                    },
                ],
            },
        );

        engine.process_position_operation(&result);

        let yes_position = cache
            .borrow()
            .position(&netting_position_id(&yes.id(), strategy))
            .unwrap()
            .cloned();
        let no_position = cache
            .borrow()
            .position(&netting_position_id(&no.id(), strategy))
            .unwrap()
            .cloned();
        assert!(yes_position.is_closed());
        assert!(no_position.is_closed());
        assert_eq!(
            yes_position.realized_pnl,
            Some(nautilus_model::types::Money::new(60.0, Currency::pUSD()))
        );
        assert_eq!(
            no_position.realized_pnl,
            Some(nautilus_model::types::Money::new(-60.0, Currency::pUSD()))
        );
    }

    #[rstest]
    fn test_restore_position_operation_barriers_from_cache() {
        let yes = option("100", "Yes");
        let (engine, cache) = engine_with(std::slice::from_ref(&yes));
        let pending_id = UUID4::new();
        let applied_id = UUID4::new();

        let record = |operation_id: UUID4, application_status: &str, venue_status: &str| {
            serde_json::json!({
                "operation_id": operation_id,
                "application_status": application_status,
                "venue_status": venue_status,
                "barrier_instrument_ids": [yes.id()],
            })
        };

        for (operation_id, application_status, venue_status) in [
            (pending_id, "Pending", "Submitted"),
            (applied_id, "Rejected", "Failed"),
        ] {
            let bytes = serde_json::to_vec(&record(operation_id, application_status, venue_status))
                .unwrap();
            cache
                .borrow_mut()
                .add(&format!("position-operation:{operation_id}"), bytes.into())
                .unwrap();
        }

        engine.restore_position_operation_barriers();

        assert_eq!(
            engine.position_operation_barrier(&yes.id()),
            Some(pending_id)
        );
    }

    #[rstest]
    fn test_restore_position_operation_barriers_includes_convert_outputs() {
        let no_a = option("720", "No");
        let no_b = option("721", "No");
        let yes_c = option("730", "Yes");
        let (engine, cache) = engine_with(&[no_a.clone(), no_b.clone(), yes_c.clone()]);
        let operation_id = UUID4::new();
        let record = serde_json::json!({
            "operation_id": operation_id,
            "application_status": "Pending",
            "venue_status": "Submitted",
            "barrier_instrument_ids": [no_a.id(), no_b.id(), yes_c.id()],
        });
        cache
            .borrow_mut()
            .add(
                &format!("position-operation:{operation_id}"),
                serde_json::to_vec(&record).unwrap().into(),
            )
            .unwrap();

        engine.restore_position_operation_barriers();

        assert_eq!(
            engine.position_operation_barrier(&no_a.id()),
            Some(operation_id)
        );
        assert_eq!(
            engine.position_operation_barrier(&no_b.id()),
            Some(operation_id)
        );
        assert_eq!(
            engine.position_operation_barrier(&yes_c.id()),
            Some(operation_id)
        );
    }

    #[rstest]
    fn test_instrument_close_is_deferred_until_operation_barrier_releases() {
        let yes = option("740", "Yes");
        let no = option("741", "No");
        let (engine, cache) = engine_with(&[yes.clone(), no]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&yes, strategy, account, Quantity::from("100"), dec!(40));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let operation_id = UUID4::new();
        engine
            .establish_full_barrier(
                operation_id,
                PositionOperationType::Split,
                &[yes.id()],
                None,
            )
            .unwrap();
        engine.settle_instrument_close(&InstrumentClose::new(
            yes.id(),
            Price::from("1"),
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(10),
            UnixNanos::from(10),
        ));
        engine.settle_instrument_close(&InstrumentClose::new(
            yes.id(),
            Price::from("0"),
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(11),
            UnixNanos::from(11),
        ));

        let position_id = netting_position_id(&yes.id(), strategy);
        assert!(!cache.borrow().position(&position_id).unwrap().is_closed());

        engine.release_position_operation_barrier(operation_id, &[yes.id()]);
        let settled = cache.borrow().position(&position_id).unwrap().cloned();
        assert!(settled.is_closed());
        assert_eq!(
            settled.realized_pnl,
            Some(nautilus_model::types::Money::new(60.0, Currency::pUSD()))
        );
    }

    #[rstest]
    fn test_barrier_rechecks_input_quantity_after_metadata_resolution() {
        let no = option("770", "No");
        let yes = option("771", "Yes");
        let (engine, cache) = engine_with(&[no.clone(), yes.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&no, strategy, account, Quantity::from("50"), dec!(20));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let operation_id = UUID4::new();
        let error = engine
            .establish_full_barrier(
                operation_id,
                PositionOperationType::Convert,
                &[no.id(), yes.id()],
                Some(&BarrierInputs {
                    strategy_id: strategy,
                    account_id: account,
                    quantity: Some(Quantity::from("100")),
                    instrument_ids: vec![no.id()],
                }),
            )
            .unwrap_err();

        assert!(error.to_string().contains("requires 100 available"));
        assert_eq!(engine.position_operation_barrier(&no.id()), None);
        assert_eq!(engine.position_operation_barrier(&yes.id()), None);
    }

    #[rstest]
    fn test_barrier_does_not_count_short_quantity_as_convert_input() {
        let no = option("773", "No");
        let yes = option("774", "Yes");
        let (engine, cache) = engine_with(&[no.clone(), yes.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = short_position(&no, strategy, account, Quantity::from("100"));
        assert!(position.is_short());
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let error = engine
            .establish_full_barrier(
                UUID4::new(),
                PositionOperationType::Convert,
                &[no.id(), yes.id()],
                Some(&BarrierInputs {
                    strategy_id: strategy,
                    account_id: account,
                    quantity: Some(Quantity::from("100")),
                    instrument_ids: vec![no.id()],
                }),
            )
            .unwrap_err();

        assert!(error.to_string().contains("requires 100 available"));
        assert_eq!(engine.position_operation_barrier(&no.id()), None);
    }

    #[rstest]
    fn test_barrier_redeem_checks_unredeemed_settled_quantity() {
        let yes = option("772", "Yes");
        let (engine, cache) = engine_with(std::slice::from_ref(&yes));
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let mut position = input_position(&yes, strategy, account, Quantity::from("100"), dec!(40));
        position
            .apply_instrument_close(InstrumentClose::new(
                yes.id(),
                Price::from("1"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(5),
                UnixNanos::from(5),
            ))
            .unwrap();
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();
        cache
            .borrow_mut()
            .add(
                &settled_consumption_key(position.id, position.ts_opened),
                bytes::Bytes::from_static(b"40"),
            )
            .unwrap();

        let input = BarrierInputs {
            strategy_id: strategy,
            account_id: account,
            quantity: Some(Quantity::from("50")),
            instrument_ids: vec![yes.id()],
        };
        assert!(
            engine
                .establish_full_barrier(
                    UUID4::new(),
                    PositionOperationType::Convert,
                    &[yes.id()],
                    Some(&input),
                )
                .is_err(),
            "convert must not draw on a settled position"
        );

        let mut excessive = BarrierInputs {
            strategy_id: strategy,
            account_id: account,
            quantity: Some(Quantity::from("61")),
            instrument_ids: vec![yes.id()],
        };
        assert!(
            engine
                .establish_full_barrier(
                    UUID4::new(),
                    PositionOperationType::Redeem,
                    &[yes.id()],
                    Some(&excessive),
                )
                .is_err(),
            "redeem must not exceed the unredeemed quantity"
        );

        excessive.quantity = Some(Quantity::from("60"));
        let operation_id = UUID4::new();
        engine
            .establish_full_barrier(
                operation_id,
                PositionOperationType::Redeem,
                &[yes.id()],
                Some(&excessive),
            )
            .unwrap();
        assert_eq!(
            engine.position_operation_barrier(&yes.id()),
            Some(operation_id)
        );
    }

    #[rstest]
    fn test_convert_rejects_settled_output_before_operation() {
        let no = option("750", "No");
        let yes = option("751", "Yes");
        let (mut engine, cache) = engine_with(&[no.clone(), yes.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&no, strategy, account, Quantity::from("100"), dec!(40));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();
        cache
            .borrow_mut()
            .add_instrument_close(InstrumentClose::new(
                yes.id(),
                Price::from("0"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(5),
                UnixNanos::from(5),
            ))
            .unwrap();

        let result = result(
            PositionOperationType::Convert,
            vec![no.id(), yes.id()],
            PositionOperationEffects {
                received: vec![amount("751", "100")],
                spent: vec![amount("750", "100")],
                collateral_delta: "0".into(),
                payouts: Vec::new(),
            },
        );
        engine.process_position_operation(&result);

        // The ample input position isolates the settled-output rejection: without the check the
        // convert would otherwise apply successfully.
        let cache = cache.borrow();
        let no_position = cache
            .position(&netting_position_id(&no.id(), strategy))
            .expect("input position must remain");
        assert!(!no_position.is_closed());
        assert_eq!(no_position.quantity, Quantity::from("100"));
        assert!(
            cache
                .position(&netting_position_id(&yes.id(), strategy))
                .is_none()
        );
    }

    #[rstest]
    fn test_restore_barrier_for_applied_record_requires_verification() {
        let yes = option("760", "Yes");
        let (engine, cache) = engine_with(std::slice::from_ref(&yes));
        let operation_id = UUID4::new();
        let record = serde_json::json!({
            "operation_id": operation_id,
            "application_status": "Applied",
            "venue_status": "Confirmed",
            "barrier_instrument_ids": [yes.id()],
        });
        cache
            .borrow_mut()
            .add(
                &format!("position-operation:{operation_id}"),
                serde_json::to_vec(&record).unwrap().into(),
            )
            .unwrap();

        engine.restore_position_operation_barriers();

        assert_eq!(
            engine.position_operation_barrier(&yes.id()),
            Some(operation_id)
        );
    }

    #[rstest]
    fn test_v2_redeem_supports_partial_settled_physical_quantity() {
        let yes = option("570", "Yes");
        let (mut engine, cache) = engine_with(std::slice::from_ref(&yes));
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let mut position = input_position(&yes, strategy, account, Quantity::from("100"), dec!(40));
        position
            .apply_instrument_close(InstrumentClose::new(
                yes.id(),
                Price::from("1"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(5),
                UnixNanos::from(5),
            ))
            .unwrap();
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();
        let position_id = netting_position_id(&yes.id(), strategy);

        let result = result(
            PositionOperationType::Redeem,
            vec![yes.id()],
            PositionOperationEffects {
                received: Vec::new(),
                spent: vec![amount("570", "40")],
                collateral_delta: "40".into(),
                payouts: Vec::new(),
            },
        );
        engine.process_position_operation(&result);

        let after = cache.borrow().position(&position_id).unwrap().cloned();
        assert_eq!(after, position);
        let consumed = {
            let cache = cache.borrow();
            settled_consumed(&cache, &cache.position(&position_id).unwrap())
        };
        assert_eq!(consumed, dec!(40));
        assert!(replay_legs(&cache, result.operation_id).is_empty());
    }

    #[rstest]
    fn test_convert_rejects_effects_outside_the_prepared_scope() {
        let no = option("770", "No");
        let yes = option("771", "Yes");
        let extra = option("772", "Yes");
        let (mut engine, cache) = engine_with(&[no.clone(), yes.clone(), extra.clone()]);
        let strategy = StrategyId::from("STRATEGY-001");
        let account = AccountId::from("ACCOUNT-001");
        let position = input_position(&no, strategy, account, Quantity::from("100"), dec!(40));
        cache
            .borrow_mut()
            .add_position_without_order(&position, OmsType::Netting)
            .unwrap();

        let result = result(
            PositionOperationType::Convert,
            vec![no.id(), yes.id()],
            PositionOperationEffects {
                received: vec![amount("771", "100"), amount("772", "100")],
                spent: vec![amount("770", "100")],
                collateral_delta: "0".into(),
                payouts: Vec::new(),
            },
        );
        engine.process_position_operation(&result);

        assert!(
            cache
                .borrow()
                .position(&netting_position_id(&extra.id(), strategy))
                .is_none()
        );
        assert!(
            cache
                .borrow()
                .position(&netting_position_id(&yes.id(), strategy))
                .is_none()
        );
    }

    #[rstest]
    fn test_overlapping_recoveries_keep_each_operations_barrier() {
        let yes = option("780", "Yes");
        let (engine, cache) = engine_with(std::slice::from_ref(&yes));
        let applied = UUID4::new();
        let pending = UUID4::new();
        for (operation_id, status) in [(applied, "Applied"), (pending, "Pending")] {
            let record = serde_json::json!({
                "operation_id": operation_id,
                "application_status": status,
                "venue_status": "Confirmed",
                "barrier_instrument_ids": [yes.id()],
            });
            cache
                .borrow_mut()
                .add(
                    &format!("position-operation:{operation_id}"),
                    serde_json::to_vec(&record).unwrap().into(),
                )
                .unwrap();
        }

        engine.restore_position_operation_barriers();
        assert_eq!(
            engine
                .position_operation_barriers
                .borrow()
                .get(&yes.id())
                .unwrap()
                .len(),
            2
        );

        engine.release_position_operation_barrier(applied, &[yes.id()]);
        assert_eq!(engine.position_operation_barrier(&yes.id()), Some(pending));

        engine.release_position_operation_barrier(pending, &[yes.id()]);
        assert_eq!(engine.position_operation_barrier(&yes.id()), None);
    }

    #[rstest]
    fn test_barrier_rejects_settled_convert_scope_but_allows_redeem() {
        let yes = option("790", "Yes");
        let (engine, cache) = engine_with(std::slice::from_ref(&yes));
        cache
            .borrow_mut()
            .add_instrument_close(InstrumentClose::new(
                yes.id(),
                Price::from("1"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(5),
                UnixNanos::from(5),
            ))
            .unwrap();

        assert!(
            engine
                .establish_full_barrier(
                    UUID4::new(),
                    PositionOperationType::Convert,
                    &[yes.id()],
                    None,
                )
                .is_err()
        );
        assert!(
            engine
                .establish_full_barrier(
                    UUID4::new(),
                    PositionOperationType::Redeem,
                    &[yes.id()],
                    None,
                )
                .is_ok()
        );
    }

    #[rstest]
    fn test_allocate_preserves_total_with_remainder() {
        let shares = allocate(dec!(100), &[dec!(1), dec!(1), dec!(1)]);
        assert_eq!(shares[0], dec!(33.333333333333333333333333333));
        assert_eq!(shares[0] + shares[1] + shares[2], dec!(100));
    }

    #[rstest]
    fn test_allocate_zero_weights_splits_evenly() {
        let shares = allocate(dec!(10), &[dec!(0), dec!(0)]);
        assert_eq!(shares, vec![dec!(5), dec!(5)]);
    }

    #[rstest]
    fn test_allocate_negative_total_keeps_sign() {
        let shares = allocate(dec!(-50), &[dec!(3), dec!(1)]);
        assert_eq!(shares[0], dec!(-37.5));
        assert_eq!(shares[1], dec!(-12.5));
        assert_eq!(shares[0] + shares[1], dec!(-50));
    }
}

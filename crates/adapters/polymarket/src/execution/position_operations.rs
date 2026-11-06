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

//! Managed position operation submission for the Polymarket execution client.

use std::sync::Arc;

use alloy_primitives::U256;
use nautilus_common::messages::execution::{
    PositionOperationControlReply, PositionOperationControlRequest,
    PositionOperationEffects as EventEffectSummary, PositionOperationRecovery,
    PositionOperationRecoveryFailed, PositionOperationRecoveryLeg,
    PositionOperationRecoveryUnresolved, PositionOperationResult, PositionOperationStartupRecovery,
    PositionOperationStatus, PositionOperationTokenAmount, PositionOperationTokenPayout,
    SubmitPositionOperation,
};
use nautilus_core::{UUID4, collections::AtomicMap, time::get_atomic_clock_realtime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    position_operation::PositionOperationType,
    types::Quantity,
};
use ustr::Ustr;

use super::PolymarketExecutionClient;
use crate::positions::{
    ManagedPositionOperationRequest, PolymarketPositionClient, PolymarketPositionOutcome,
    amounts::base_units_to_pusd,
    effects::PositionOperationEffects,
    is_ambiguous_submit_error,
    record::{
        ManagedOperationIntent, PositionOperationRecord, PositionOperationRecordGate,
        PositionOperationRecordStore, PositionOperationStoreRecorder,
    },
};

/// Record store that mirrors updates into the node Cache and retains an in-process copy.
#[derive(Debug)]
pub(crate) struct CorePositionOperationStore {
    emitter: ExecutionEventEmitter,
    records: std::sync::Mutex<std::collections::HashMap<UUID4, PositionOperationRecord>>,
}

impl CorePositionOperationStore {
    pub(crate) fn new(emitter: ExecutionEventEmitter) -> Self {
        Self {
            emitter,
            records: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Seeds the in-memory index with records recovered from the persistent cache.
    pub(crate) fn restore(&self, records: Vec<PositionOperationRecord>) -> anyhow::Result<()> {
        let mut guard = self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("position operation store lock poisoned"))?;

        for record in records {
            guard.insert(record.operation_id, record);
        }
        Ok(())
    }
}

impl PositionOperationRecordStore for CorePositionOperationStore {
    fn load(&self, operation_id: &UUID4) -> anyhow::Result<Option<PositionOperationRecord>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("position operation store lock poisoned"))?
            .get(operation_id)
            .cloned())
    }

    fn save(&self, record: &PositionOperationRecord) -> anyhow::Result<()> {
        self.records
            .lock()
            .map_err(|_| anyhow::anyhow!("position operation store lock poisoned"))?
            .insert(record.operation_id, record.clone());

        let value = serde_json::to_value(record)?;
        let key = PositionOperationRecord::key(&record.operation_id);
        let reply: PositionOperationControlReply = Arc::new(|_| {});
        self.emitter
            .send_position_operation_control(PositionOperationControlRequest::Record {
                key,
                record: value,
                reply,
            });
        Ok(())
    }

    fn restore(&self, records: Vec<PositionOperationRecord>) -> anyhow::Result<()> {
        Self::restore(self, records)
    }

    fn records(&self) -> anyhow::Result<Vec<PositionOperationRecord>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("position operation store lock poisoned"))?
            .values()
            .cloned()
            .collect())
    }
}

/// Builds the Live recorder backed by the node Cache.
pub(crate) fn core_position_operation_recorder(
    store: Arc<CorePositionOperationStore>,
) -> Arc<PositionOperationStoreRecorder> {
    Arc::new(PositionOperationStoreRecorder::new(store))
}

/// Record gate that hands the prepared operation to the Cache-owning execution thread.
#[derive(Debug)]
pub(crate) struct CorePositionOperationRecordGate {
    pub(crate) emitter: ExecutionEventEmitter,
}

#[async_trait::async_trait]
impl PositionOperationRecordGate for CorePositionOperationRecordGate {
    async fn confirm_prepared(&self, record: &PositionOperationRecord) -> anyhow::Result<()> {
        let value = serde_json::to_value(record)?;
        let key = PositionOperationRecord::key(&record.operation_id);
        request_control(&self.emitter, |reply| {
            PositionOperationControlRequest::Record {
                key,
                record: value,
                reply,
            }
        })
        .await
    }
}

async fn request_control(
    emitter: &ExecutionEventEmitter,
    build: impl FnOnce(PositionOperationControlReply) -> PositionOperationControlRequest,
) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(std::sync::Mutex::new(Some(sender)));
    let reply: PositionOperationControlReply = Arc::new(move |result| {
        if let Ok(mut slot) = sender.lock()
            && let Some(sender) = slot.take()
        {
            let _ = sender.send(result);
        }
    });

    emitter.send_position_operation_control(build(reply));
    let result = receiver
        .await
        .map_err(|_| anyhow::anyhow!("position operation control reply channel closed"))?;
    result.map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

async fn request_position_operation_barrier(
    emitter: &ExecutionEventEmitter,
    operation_id: UUID4,
    operation: PositionOperationType,
    instrument_ids: &[InstrumentId],
    inputs: &BarrierInputRequest<'_>,
) -> anyhow::Result<()> {
    request_control(emitter, |reply| PositionOperationControlRequest::Barrier {
        operation_id,
        operation,
        instrument_ids: instrument_ids.to_vec(),
        strategy_id: inputs.strategy_id,
        account_id: inputs.account_id,
        quantity: inputs.quantity,
        input_instrument_ids: inputs.instrument_ids.to_vec(),
        reply,
    })
    .await
}

struct BarrierInputRequest<'a> {
    strategy_id: StrategyId,
    account_id: AccountId,
    quantity: Option<Quantity>,
    instrument_ids: &'a [InstrumentId],
}

#[derive(Debug)]
struct InstrumentOperationMetadata {
    condition_id: String,
    token_id: String,
    outcome: String,
    market_id: Option<String>,
}

enum ResolvedOperation {
    Ready {
        intent: ManagedOperationIntent,
        request: ManagedPositionOperationRequest,
        /// CTF redeem token IDs mapped to their condition outcome index.
        payout_indices: Vec<(String, u8)>,
    },
    Convert {
        intent: ManagedOperationIntent,
        market_id: String,
        token_ids: Vec<String>,
        amount: rust_decimal::Decimal,
    },
}

impl PolymarketExecutionClient {
    /// Loads persisted operation records and restores the wallet nonce reservation.
    ///
    /// Recovery never re-signs or resubmits. Records whose outcome remains unknown keep the
    /// reservation, so the wallet rejects further operations until evidence is available.
    pub(crate) fn restore_position_operations(&self) -> anyhow::Result<()> {
        let Some(position_client) = self.position_client.clone() else {
            return Ok(());
        };

        let records = {
            let cache = self.core.cache();
            crate::positions::record::load_position_operation_records(&cache)?
        };

        if records.is_empty() {
            return Ok(());
        }

        position_client.restore_position_operation_records(records.clone())?;

        let latest = records
            .iter()
            .filter_map(|record| {
                let nonce = record
                    .unsigned_nonce
                    .as_deref()
                    .and_then(|value| U256::from_str_radix(value, 10).ok())?;
                Some((nonce, record))
            })
            .max_by(|(left, _), (right, _)| left.cmp(right));

        if let Some((nonce, record)) = latest {
            position_client.restore_submission(nonce, record.venue_transaction_id.clone());
        }

        for record in records.iter().filter(|record| record.requires_recovery()) {
            log::warn!(
                "Recovered unresolved position operation {} (operation={:?}, venue={:?}, application={:?}, transaction_id={:?})",
                record.operation_id,
                record.operation,
                record.venue_status,
                record.application_status,
                record.venue_transaction_id,
            );
        }

        Ok(())
    }

    /// Queries original Relayer transactions and prepares startup recovery actions.
    ///
    /// This never re-signs or resubmits. Prepared actions are taken by the core after all
    /// execution clients have connected and before ordinary venue reconciliation.
    pub(crate) async fn prepare_startup_recovery_inner(&mut self) -> anyhow::Result<()> {
        self.startup_recovery.clear();

        let Some(position_client) = self.position_client.clone() else {
            return Ok(());
        };
        let records = position_client.position_operation_records()?;

        for record in records
            .into_iter()
            .filter(PositionOperationRecord::requires_recovery)
        {
            let prepared = self
                .prepare_operation_recovery(&position_client, &record)
                .await;
            self.startup_recovery.push(prepared);
        }

        Ok(())
    }

    async fn prepare_operation_recovery(
        &self,
        position_client: &PolymarketPositionClient,
        record: &PositionOperationRecord,
    ) -> PositionOperationStartupRecovery {
        let Some(transaction_id) = record.venue_transaction_id.clone() else {
            return unresolved_without_transaction_id(record);
        };

        match position_client
            .resume_transaction(&transaction_id)
            .wait()
            .await
        {
            Ok(PolymarketPositionOutcome::Confirmed {
                transaction_hash, ..
            }) => {
                let effects = self
                    .recovery_effects(position_client, record, transaction_hash.as_deref())
                    .await;

                match effects {
                    Some(effects) => PositionOperationStartupRecovery::Reconcile(Box::new(
                        PositionOperationRecovery {
                            trader_id: record.trader_id,
                            strategy_id: record.strategy_id,
                            client_id: self.core.client_id,
                            account_id: self.core.account_id,
                            operation_id: record.operation_id,
                            operation: record.operation,
                            instrument_ids: record.barrier_instrument_ids.clone(),
                            quantity: record.quantity,
                            effects: Some(effects),
                            venue_transaction_id: Some(transaction_id),
                            transaction_hash,
                            applied_legs: record
                                .applied_legs
                                .iter()
                                .map(|leg| PositionOperationRecoveryLeg {
                                    position_id: leg.position_id,
                                    instrument_id: leg.instrument_id,
                                    strategy_id: leg.strategy_id,
                                    direction: leg.direction,
                                    quantity: leg.quantity,
                                    cost: leg.cost,
                                    income: leg.income,
                                    settled: leg.settled,
                                })
                                .collect(),
                        },
                    )),
                    None => PositionOperationStartupRecovery::Unresolved(Box::new(
                        PositionOperationRecoveryUnresolved {
                            operation_id: record.operation_id,
                            venue_status: PositionOperationStatus::Confirmed,
                            transaction_id: Some(transaction_id),
                            transaction_hash,
                            message: "confirmed transaction has no readable effect evidence".into(),
                        },
                    )),
                }
            }
            Ok(PolymarketPositionOutcome::Failed {
                transaction_hash,
                error_msg,
                ..
            }) => PositionOperationStartupRecovery::Failed(Box::new(
                PositionOperationRecoveryFailed {
                    operation_id: record.operation_id,
                    instrument_ids: record.barrier_instrument_ids.clone(),
                    transaction_id: Some(transaction_id),
                    transaction_hash,
                    message: error_msg.unwrap_or_else(|| "Relayer reported failure".to_string()),
                },
            )),
            Ok(PolymarketPositionOutcome::Invalid { error_msg, .. }) => {
                PositionOperationStartupRecovery::Failed(Box::new(
                    PositionOperationRecoveryFailed {
                        operation_id: record.operation_id,
                        instrument_ids: record.barrier_instrument_ids.clone(),
                        transaction_id: Some(transaction_id),
                        transaction_hash: None,
                        message: error_msg
                            .unwrap_or_else(|| "Relayer reported invalid".to_string()),
                    },
                ))
            }
            Err(e) => PositionOperationStartupRecovery::Unresolved(Box::new(
                PositionOperationRecoveryUnresolved {
                    operation_id: record.operation_id,
                    venue_status: PositionOperationStatus::Unknown,
                    transaction_id: Some(transaction_id),
                    transaction_hash: record.transaction_hash.clone(),
                    message: format!("failed to resolve the original transaction: {e}"),
                },
            )),
        }
    }

    async fn recovery_effects(
        &self,
        position_client: &PolymarketPositionClient,
        record: &PositionOperationRecord,
        transaction_hash: Option<&str>,
    ) -> Option<EventEffectSummary> {
        let transaction_hash = transaction_hash?;
        let mut effects = position_client
            .transaction_effects(transaction_hash)
            .await
            .ok()
            .map(effect_summary)?;

        if record.operation != PositionOperationType::Redeem {
            return Some(effects);
        }
        let Some(condition_id) = record.condition_id.as_deref() else {
            return Some(effects);
        };

        match position_client.ctf_payouts(condition_id).await {
            Ok((denominator, yes, no)) => {
                effects.payouts = effects
                    .spent
                    .iter()
                    .filter_map(|amount| {
                        let instrument = self
                            .shared_token_instruments
                            .get_cloned(&Ustr::from(amount.token_id.as_str()))?;
                        let InstrumentAny::BinaryOption(binary) = instrument else {
                            return None;
                        };
                        let outcome = binary.outcome?.to_string();
                        let numerator = if outcome.eq_ignore_ascii_case("yes") {
                            yes
                        } else if outcome.eq_ignore_ascii_case("no") {
                            no
                        } else {
                            return None;
                        };
                        Some(PositionOperationTokenPayout {
                            token_id: amount.token_id.clone(),
                            numerator: numerator.to_string(),
                            denominator: denominator.to_string(),
                        })
                    })
                    .collect();

                if effects.payouts.len() != effects.spent.len() {
                    log::warn!(
                        "Recovered CTF redeem for condition {condition_id} has incomplete payout evidence"
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to read CTF payouts for recovered redeem condition {condition_id}: {e}"
                );
            }
        }

        Some(effects)
    }

    /// Validates and submits a managed position operation.
    pub(crate) fn submit_position_operation_command(
        &self,
        cmd: &SubmitPositionOperation,
    ) -> anyhow::Result<()> {
        if let Err(e) = self.settlement.ensure_submission_allowed() {
            self.send_position_operation_result(
                cmd,
                PositionOperationStatus::Rejected,
                None,
                None,
                Some(e.to_string()),
                None,
            );
            return Ok(());
        }

        for instrument_id in &cmd.instrument_ids {
            if let Err(e) = self
                .settlement
                .ensure_operation_inputs_terminal(Some(*instrument_id), "position operation")
            {
                self.send_position_operation_result(
                    cmd,
                    PositionOperationStatus::Rejected,
                    None,
                    None,
                    Some(e.to_string()),
                    None,
                );
                return Ok(());
            }
        }

        if let Err(e) = cmd.validate() {
            self.send_position_operation_result(
                cmd,
                PositionOperationStatus::Rejected,
                None,
                None,
                Some(e.to_string()),
                None,
            );
            return Ok(());
        }

        let Some(position_client) = self.position_client.clone() else {
            self.send_position_operation_result(
                cmd,
                PositionOperationStatus::Rejected,
                None,
                None,
                Some("managed position operations are not configured".into()),
                None,
            );
            return Ok(());
        };

        let resolved = match self.resolve_operation(cmd) {
            Ok(resolved) => resolved,
            Err(e) => {
                self.send_position_operation_result(
                    cmd,
                    PositionOperationStatus::Rejected,
                    None,
                    None,
                    Some(e.to_string()),
                    None,
                );
                return Ok(());
            }
        };

        let emitter = self.emitter.clone();
        let client_id = self.core.client_id;
        let account_id = self.core.account_id;
        let task_cmd = cmd.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let settlement = self.settlement.clone();

        self.pending_tasks
            .spawn(async move {
                run_operation_task(
                    position_client,
                    resolved,
                    emitter,
                    client_id,
                    account_id,
                    token_instruments,
                    settlement,
                    task_cmd,
                )
                .await;
            })
            .map_err(|e| anyhow::anyhow!("failed to spawn position operation task: {e}"))?;
        Ok(())
    }

    fn resolve_operation(
        &self,
        cmd: &SubmitPositionOperation,
    ) -> anyhow::Result<ResolvedOperation> {
        let metadata = self.resolve_instrument_metadata(cmd)?;
        self.ensure_local_inputs(cmd, &metadata)?;
        let condition_id = metadata
            .first()
            .map(|meta| meta.condition_id.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("position operation requires at least one instrument")
            })?;

        validate_operation_scope(cmd.operation, &metadata)?;

        let intent = ManagedOperationIntent {
            operation_id: cmd.operation_id,
            trader_id: cmd.trader_id,
            strategy_id: cmd.strategy_id,
            account_id: self.core.account_id,
            operation: cmd.operation,
            instrument_ids: cmd.instrument_ids.clone(),
            barrier_instrument_ids: Vec::new(),
            quantity: cmd.quantity,
            condition_id: Some(condition_id.clone()),
            market_id: metadata.first().and_then(|meta| meta.market_id.clone()),
            index_set: None,
        };

        match cmd.operation {
            PositionOperationType::Split | PositionOperationType::Merge => {
                ensure_yes_no_roles(&metadata)?;
                let amount = required_amount(cmd)?;
                let request = if cmd.operation == PositionOperationType::Split {
                    ManagedPositionOperationRequest::Split {
                        condition_id,
                        amount,
                    }
                } else {
                    ManagedPositionOperationRequest::Merge {
                        condition_id,
                        amount,
                    }
                };
                Ok(ResolvedOperation::Ready {
                    intent,
                    request,
                    payout_indices: Vec::new(),
                })
            }
            PositionOperationType::Redeem => {
                ensure_condition_matches(&metadata)?;
                if cmd.instrument_ids.len() == 2 {
                    let payout_indices = metadata
                        .iter()
                        .map(|meta| Ok((meta.token_id.clone(), ctf_outcome_index(&meta.outcome)?)))
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    Ok(ResolvedOperation::Ready {
                        intent,
                        request: ManagedPositionOperationRequest::RedeemCtf { condition_id },
                        payout_indices,
                    })
                } else {
                    let index = outcome_index(&metadata[0].token_id)?;
                    let amount = required_amount(cmd)?;
                    Ok(ResolvedOperation::Ready {
                        intent,
                        request: ManagedPositionOperationRequest::RedeemV2 {
                            condition_id,
                            outcome_index: index,
                            amount,
                        },
                        payout_indices: Vec::new(),
                    })
                }
            }
            PositionOperationType::Convert => {
                ensure_no_roles(&metadata)?;
                let market_id = common_market_id(&metadata)?;
                let token_ids = metadata.iter().map(|meta| meta.token_id.clone()).collect();
                let amount = required_amount(cmd)?;
                Ok(ResolvedOperation::Convert {
                    intent: ManagedOperationIntent {
                        condition_id: None,
                        market_id: Some(market_id.clone()),
                        ..intent
                    },
                    market_id,
                    token_ids,
                    amount,
                })
            }
        }
    }

    fn ensure_local_inputs(
        &self,
        cmd: &SubmitPositionOperation,
        metadata: &[InstrumentOperationMetadata],
    ) -> anyhow::Result<()> {
        let amount = match cmd.operation {
            PositionOperationType::Split => return Ok(()),
            PositionOperationType::Redeem if metadata.len() == 2 => {
                // CTF redeem is wallet-scoped; its ownership is resolved on confirmed effects.
                return Ok(());
            }
            _ => required_amount(cmd)?,
        };

        let cache = self.core.cache();
        for instrument_id in &cmd.instrument_ids {
            let positions = cache.positions_open(
                Some(&self.core.venue),
                Some(instrument_id),
                Some(&cmd.strategy_id),
                Some(&self.core.account_id),
                None,
            );
            anyhow::ensure!(
                positions.len() <= 1,
                "multiple open positions for {instrument_id} in NETTING mode"
            );
            let available = positions
                .first()
                .map_or(rust_decimal::Decimal::ZERO, |position| {
                    position
                        .signed_decimal_qty()
                        .max(rust_decimal::Decimal::ZERO)
                });
            if available < amount {
                // Only a redeem may draw on an economically settled position's physical tokens.
                // The core rechecks the unredeemed remainder under the operation barrier.
                let settled = cmd.operation == PositionOperationType::Redeem
                    && cache
                        .positions(
                            None,
                            Some(instrument_id),
                            Some(&cmd.strategy_id),
                            Some(&self.core.account_id),
                            None,
                        )
                        .into_iter()
                        .any(|position| position.is_closed());
                anyhow::ensure!(
                    settled,
                    "position operation requires {amount} available {instrument_id}, found {available}"
                );
            }
        }
        Ok(())
    }

    fn resolve_instrument_metadata(
        &self,
        cmd: &SubmitPositionOperation,
    ) -> anyhow::Result<Vec<InstrumentOperationMetadata>> {
        let cache = self.core.cache();
        let mut metadata = Vec::with_capacity(cmd.instrument_ids.len());
        let mut condition_id: Option<String> = None;

        for instrument_id in &cmd.instrument_ids {
            let instrument = cache
                .instrument(instrument_id)
                .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} is not in the cache"))?;
            let InstrumentAny::BinaryOption(binary) = instrument else {
                anyhow::bail!("position operations require binary option instruments");
            };
            let info = binary
                .info
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} has no metadata"))?;
            let condition = info
                .get_str("condition_id")
                .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} has no condition_id"))?
                .to_string();
            let token_id = info
                .get_str("token_id")
                .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} has no token_id"))?
                .to_string();
            let outcome = binary
                .outcome
                .map(|outcome| outcome.to_string())
                .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} has no outcome"))?;
            let market_id = info
                .get("gamma_event")
                .and_then(|event| event.get("negRiskMarketID"))
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    info.get("gamma_market")
                        .and_then(|market| market.get("negRiskMarketID"))
                        .and_then(serde_json::Value::as_str)
                })
                .map(str::to_string);

            if condition_id.is_none() {
                condition_id = Some(condition.clone());
            }

            metadata.push(InstrumentOperationMetadata {
                condition_id: condition,
                token_id,
                outcome,
                market_id,
            });
        }

        Ok(metadata)
    }

    fn send_position_operation_result(
        &self,
        cmd: &SubmitPositionOperation,
        status: PositionOperationStatus,
        transaction_id: Option<String>,
        transaction_hash: Option<String>,
        message: Option<String>,
        effects: Option<EventEffectSummary>,
    ) {
        send_position_operation_result(
            &self.emitter,
            cmd,
            self.core.client_id,
            self.core.account_id,
            status,
            transaction_id,
            transaction_hash,
            message,
            effects,
        );
    }
}

struct ConvertRecheck {
    market_id: String,
    token_ids: Vec<String>,
    index_set: U256,
    expected_outputs: Vec<String>,
}

async fn recheck_convert_mapping(
    position_client: &PolymarketPositionClient,
    recheck: &ConvertRecheck,
) -> anyhow::Result<()> {
    let market = position_client
        .neg_risk_market(&recheck.market_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to re-verify negative-risk metadata: {e}"))?;
    let tokens = recheck
        .token_ids
        .iter()
        .map(|token| U256::from_str_radix(token, 10))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| anyhow::anyhow!("invalid conversion token id"))?;
    let index_set = market
        .index_set_for_no_tokens(&tokens)
        .map_err(|e| anyhow::anyhow!("negative-risk inputs changed before signing: {e}"))?;
    anyhow::ensure!(
        index_set.bits() == recheck.index_set,
        "negative-risk inputs changed before signing"
    );
    let outputs: Vec<String> = market
        .conditions
        .iter()
        .enumerate()
        .filter(|(index, _)| !index_set.bits().bit(*index))
        .map(|(_, condition)| condition.yes_token_id.to_string())
        .collect();
    anyhow::ensure!(
        outputs == recheck.expected_outputs,
        "negative-risk output mapping changed before signing"
    );
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "task fields mirror the routed operation identity and venue evidence"
)]
async fn run_operation_task(
    position_client: std::sync::Arc<PolymarketPositionClient>,
    resolved: ResolvedOperation,
    emitter: ExecutionEventEmitter,
    client_id: ClientId,
    account_id: AccountId,
    token_instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    settlement: Arc<crate::execution::settlement::SettlementRegistry>,
    cmd: SubmitPositionOperation,
) {
    let (mut intent, request, full_instrument_ids, payout_indices, convert_recheck) = match resolved
    {
        ResolvedOperation::Ready {
            intent,
            request,
            payout_indices,
        } => (
            intent,
            request,
            cmd.instrument_ids.clone(),
            payout_indices,
            None,
        ),
        ResolvedOperation::Convert {
            mut intent,
            market_id,
            token_ids,
            amount,
        } => {
            let market = match position_client.neg_risk_market(&market_id).await {
                Ok(market) => market,
                Err(e) => {
                    send_position_operation_result(
                        &emitter,
                        &cmd,
                        client_id,
                        account_id,
                        PositionOperationStatus::Rejected,
                        None,
                        None,
                        Some(format!("failed to verify negative-risk metadata: {e}")),
                        None,
                    );
                    return;
                }
            };

            let parsed_tokens = token_ids
                .iter()
                .map(|token| {
                    U256::from_str_radix(token, 10)
                        .map_err(|_| anyhow::anyhow!("invalid token id {token}"))
                })
                .collect::<anyhow::Result<Vec<_>>>();

            let index_set = match parsed_tokens {
                Ok(tokens) => market
                    .index_set_for_no_tokens(&tokens)
                    .map_err(|e| anyhow::anyhow!("invalid conversion inputs: {e}")),
                Err(e) => Err(e),
            };

            let index_set = match index_set {
                Ok(index_set) => index_set,
                Err(e) => {
                    send_position_operation_result(
                        &emitter,
                        &cmd,
                        client_id,
                        account_id,
                        PositionOperationStatus::Rejected,
                        None,
                        None,
                        Some(e.to_string()),
                        None,
                    );
                    return;
                }
            };

            intent.index_set = Some(index_set.bits().to_string());
            let request = ManagedPositionOperationRequest::Convert {
                market_id: market_id.clone(),
                index_set: index_set.bits(),
                amount,
            };

            // Resolve every expected YES output to a managed BinaryOption before the barrier.
            let mut full_instrument_ids = cmd.instrument_ids.clone();
            let mut missing = Vec::new();
            for (index, condition) in market.conditions.iter().enumerate() {
                if index_set.bits().bit(index) {
                    continue;
                }
                let token_id = condition.yes_token_id.to_string();
                match token_instruments.get_cloned(&Ustr::from(token_id.as_str())) {
                    Some(instrument) => full_instrument_ids.push(instrument.id()),
                    None => missing.push(token_id),
                }
            }

            if !missing.is_empty() {
                send_position_operation_result(
                    &emitter,
                    &cmd,
                    client_id,
                    account_id,
                    PositionOperationStatus::Rejected,
                    None,
                    None,
                    Some(format!(
                        "missing BinaryOption mapping for conversion outputs: {}",
                        missing.join(",")
                    )),
                    None,
                );
                return;
            }

            full_instrument_ids.sort();
            full_instrument_ids.dedup();
            let expected_outputs = market
                .conditions
                .iter()
                .enumerate()
                .filter(|(index, _)| !index_set.bits().bit(*index))
                .map(|(_, condition)| condition.yes_token_id.to_string())
                .collect();
            (
                intent,
                request,
                full_instrument_ids,
                Vec::new(),
                Some(ConvertRecheck {
                    market_id,
                    token_ids,
                    index_set: index_set.bits(),
                    expected_outputs,
                }),
            )
        }
    };

    intent.barrier_instrument_ids = full_instrument_ids.clone();
    let condition_id = intent.condition_id.clone();
    let operation_id = intent.operation_id;

    let mut result_cmd = cmd.clone();
    result_cmd.instrument_ids = full_instrument_ids.clone();

    let barrier_inputs = BarrierInputRequest {
        strategy_id: cmd.strategy_id,
        account_id,
        quantity: cmd.quantity,
        instrument_ids: &cmd.instrument_ids,
    };
    if let Err(e) = request_position_operation_barrier(
        &emitter,
        operation_id,
        request.operation(),
        &full_instrument_ids,
        &barrier_inputs,
    )
    .await
    {
        send_position_operation_result(
            &emitter,
            &result_cmd,
            client_id,
            account_id,
            PositionOperationStatus::Rejected,
            None,
            None,
            Some(format!("position operation barrier rejected: {e}")),
            None,
        );
        return;
    }

    if let Some(recheck) = convert_recheck
        && let Err(e) = recheck_convert_mapping(&position_client, &recheck).await
    {
        send_position_operation_result(
            &emitter,
            &result_cmd,
            client_id,
            account_id,
            PositionOperationStatus::Rejected,
            None,
            None,
            Some(e.to_string()),
            None,
        );
        return;
    }

    for instrument_id in &full_instrument_ids {
        if let Err(e) =
            settlement.ensure_operation_inputs_terminal(Some(*instrument_id), "position operation")
        {
            send_position_operation_result(
                &emitter,
                &result_cmd,
                client_id,
                account_id,
                PositionOperationStatus::Rejected,
                None,
                None,
                Some(e.to_string()),
                None,
            );
            return;
        }
    }

    match position_client
        .submit_managed_position_operation(intent, request)
        .await
    {
        Ok(transaction) => {
            let transaction_id = transaction.transaction_id().to_string();
            send_position_operation_result(
                &emitter,
                &result_cmd,
                client_id,
                account_id,
                PositionOperationStatus::Submitted,
                Some(transaction_id.clone()),
                None,
                None,
                None,
            );

            match transaction.wait().await {
                Ok(PolymarketPositionOutcome::Confirmed {
                    transaction_hash, ..
                }) => {
                    let mut effects = match &transaction_hash {
                        Some(hash) => position_client
                            .transaction_effects(hash)
                            .await
                            .ok()
                            .map(effect_summary),
                        None => None,
                    };

                    // A CTF redeem burns both outcomes. The per-outcome payout is required before
                    // the aggregate collateral can be allocated by the core.
                    if let (Some(summary), Some(condition_id)) =
                        (effects.as_mut(), condition_id.as_deref())
                        && !payout_indices.is_empty()
                    {
                        match position_client.ctf_payouts(condition_id).await {
                            Ok((denominator, yes, no)) => {
                                summary.payouts = payout_indices
                                    .iter()
                                    .map(|(token_id, index)| {
                                        let numerator = if *index == 0 { yes } else { no };
                                        PositionOperationTokenPayout {
                                            token_id: token_id.clone(),
                                            numerator: numerator.to_string(),
                                            denominator: denominator.to_string(),
                                        }
                                    })
                                    .collect();
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to read CTF payouts for condition {condition_id}: {e}; position operation application will remain unresolved"
                                );
                            }
                        }
                    }

                    send_position_operation_result(
                        &emitter,
                        &result_cmd,
                        client_id,
                        account_id,
                        PositionOperationStatus::Confirmed,
                        Some(transaction_id),
                        transaction_hash,
                        None,
                        effects,
                    );
                }
                Ok(PolymarketPositionOutcome::Failed {
                    transaction_hash,
                    error_msg,
                    ..
                }) => {
                    send_position_operation_result(
                        &emitter,
                        &result_cmd,
                        client_id,
                        account_id,
                        PositionOperationStatus::Failed,
                        Some(transaction_id),
                        transaction_hash,
                        error_msg,
                        None,
                    );
                }
                Ok(PolymarketPositionOutcome::Invalid { error_msg, .. }) => {
                    send_position_operation_result(
                        &emitter,
                        &result_cmd,
                        client_id,
                        account_id,
                        PositionOperationStatus::Failed,
                        Some(transaction_id),
                        None,
                        error_msg,
                        None,
                    );
                }
                Err(e) => {
                    send_position_operation_result(
                        &emitter,
                        &result_cmd,
                        client_id,
                        account_id,
                        PositionOperationStatus::Unknown,
                        Some(transaction_id),
                        None,
                        Some(e.to_string()),
                        None,
                    );
                }
            }
        }
        Err(e) => {
            let status = if is_ambiguous_submit_error(&e) {
                PositionOperationStatus::Unknown
            } else {
                PositionOperationStatus::Rejected
            };
            send_position_operation_result(
                &emitter,
                &result_cmd,
                client_id,
                account_id,
                status,
                None,
                None,
                Some(e.to_string()),
                None,
            );
        }
    }
}

fn unresolved_without_transaction_id(
    record: &PositionOperationRecord,
) -> PositionOperationStartupRecovery {
    PositionOperationStartupRecovery::Unresolved(Box::new(PositionOperationRecoveryUnresolved {
        operation_id: record.operation_id,
        venue_status: PositionOperationStatus::Unknown,
        transaction_id: None,
        transaction_hash: record.transaction_hash.clone(),
        message: "no transaction identifier or available evidence is sufficient to reconcile"
            .into(),
    }))
}

#[expect(
    clippy::too_many_arguments,
    reason = "result fields mirror the routed operation identity and venue evidence"
)]
fn send_position_operation_result(
    emitter: &ExecutionEventEmitter,
    cmd: &SubmitPositionOperation,
    client_id: ClientId,
    account_id: AccountId,
    status: PositionOperationStatus,
    transaction_id: Option<String>,
    transaction_hash: Option<String>,
    message: Option<String>,
    effects: Option<EventEffectSummary>,
) {
    let ts = get_atomic_clock_realtime().get_time_ns();
    emitter.send_position_operation_result(PositionOperationResult::new(
        cmd.trader_id,
        cmd.strategy_id,
        client_id,
        account_id,
        cmd.operation_id,
        cmd.operation,
        status,
        cmd.instrument_ids.clone(),
        transaction_id,
        transaction_hash,
        message,
        effects,
        UUID4::new(),
        ts,
        ts,
    ));
}

fn effect_summary(effects: PositionOperationEffects) -> EventEffectSummary {
    let convert = |amounts: Vec<crate::positions::effects::TokenAmount>| {
        amounts
            .into_iter()
            .map(|token| PositionOperationTokenAmount {
                token_id: token.token_id.to_string(),
                quantity: base_units_string(token.quantity),
            })
            .collect()
    };

    let collateral_delta = if effects.collateral_delta.is_negative() {
        format!(
            "-{}",
            base_units_string(effects.collateral_delta.unsigned_abs())
        )
    } else {
        base_units_string(effects.collateral_delta.unsigned_abs())
    };

    EventEffectSummary {
        received: convert(effects.received),
        spent: convert(effects.spent),
        collateral_delta,
        payouts: Vec::new(),
    }
}

fn base_units_string(amount: U256) -> String {
    base_units_to_pusd(amount).map_or_else(|_| amount.to_string(), |value| value.to_string())
}

fn required_amount(cmd: &SubmitPositionOperation) -> anyhow::Result<rust_decimal::Decimal> {
    cmd.quantity
        .map(|quantity| quantity.as_decimal())
        .ok_or_else(|| anyhow::anyhow!("position operation requires a quantity"))
}

fn validate_operation_scope(
    operation: PositionOperationType,
    metadata: &[InstrumentOperationMetadata],
) -> anyhow::Result<()> {
    if operation == PositionOperationType::Convert {
        common_market_id(metadata).map(|_| ())
    } else {
        ensure_condition_matches(metadata)
    }
}

fn ensure_condition_matches(metadata: &[InstrumentOperationMetadata]) -> anyhow::Result<()> {
    anyhow::ensure!(
        metadata
            .windows(2)
            .all(|pair| pair[0].condition_id == pair[1].condition_id),
        "position operation instruments span different conditions"
    );
    Ok(())
}

fn ensure_yes_no_roles(metadata: &[InstrumentOperationMetadata]) -> anyhow::Result<()> {
    anyhow::ensure!(
        metadata.len() == 2,
        "split and merge require both outcome instruments"
    );
    ensure_condition_matches(metadata)?;
    let has_yes = metadata
        .iter()
        .any(|meta| meta.outcome.eq_ignore_ascii_case("yes"));
    let has_no = metadata
        .iter()
        .any(|meta| meta.outcome.eq_ignore_ascii_case("no"));
    anyhow::ensure!(
        has_yes && has_no,
        "split and merge require YES and NO outcomes"
    );
    Ok(())
}

fn ensure_no_roles(metadata: &[InstrumentOperationMetadata]) -> anyhow::Result<()> {
    anyhow::ensure!(
        metadata
            .iter()
            .all(|meta| meta.outcome.eq_ignore_ascii_case("no")),
        "conversion inputs must be NO instruments"
    );
    Ok(())
}

fn common_market_id(metadata: &[InstrumentOperationMetadata]) -> anyhow::Result<String> {
    let market_id = metadata
        .first()
        .and_then(|meta| meta.market_id.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "instrument metadata is missing the negative-risk market ID required for conversion"
            )
        })?;
    anyhow::ensure!(
        metadata
            .iter()
            .all(|meta| meta.market_id.as_deref() == Some(market_id.as_str())),
        "conversion instruments span different negative-risk markets"
    );
    Ok(market_id)
}

fn ctf_outcome_index(outcome: &str) -> anyhow::Result<u8> {
    if outcome.eq_ignore_ascii_case("yes") {
        Ok(0)
    } else if outcome.eq_ignore_ascii_case("no") {
        Ok(1)
    } else {
        anyhow::bail!("unsupported CTF outcome label {outcome}")
    }
}

fn outcome_index(token_id: &str) -> anyhow::Result<u8> {
    let token_id = U256::from_str_radix(token_id, 10)
        .map_err(|_| anyhow::anyhow!("invalid V2 position token id {token_id}"))?;
    let index = u8::try_from(token_id & U256::from(0xFFu8))
        .map_err(|_| anyhow::anyhow!("V2 position token index is out of range"))?;
    anyhow::ensure!(index <= 1, "V2 position token index must be 0 or 1");
    Ok(index)
}

#[cfg(test)]
mod tests {
    use alloy_primitives::I256;
    use rstest::rstest;

    use super::*;
    use crate::positions::effects::TokenAmount;

    fn metadata(
        condition: &str,
        outcome: &str,
        token_id: &str,
        market_id: Option<&str>,
    ) -> InstrumentOperationMetadata {
        InstrumentOperationMetadata {
            condition_id: condition.into(),
            token_id: token_id.into(),
            outcome: outcome.into(),
            market_id: market_id.map(str::to_string),
        }
    }

    #[rstest]
    fn test_unresolved_without_transaction_id_keeps_unknown() {
        use nautilus_core::UnixNanos;
        use nautilus_model::{
            identifiers::{StrategyId, TraderId},
            types::Quantity,
        };

        let operation_id = UUID4::new();
        let record = PositionOperationRecord::prepared(
            &ManagedOperationIntent {
                operation_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("STRATEGY-001"),
                account_id: AccountId::from("POLYMARKET-001"),
                operation: PositionOperationType::Convert,
                instrument_ids: vec![InstrumentId::from("1001.POLYMARKET")],
                barrier_instrument_ids: vec![InstrumentId::from("1001.POLYMARKET")],
                quantity: Some(Quantity::from("100")),
                condition_id: None,
                market_id: Some("0xmarket".into()),
                index_set: Some("1".into()),
            },
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );

        assert!(record.requires_recovery());

        match unresolved_without_transaction_id(&record) {
            PositionOperationStartupRecovery::Unresolved(unresolved) => {
                assert_eq!(unresolved.operation_id, operation_id);
                assert_eq!(unresolved.venue_status, PositionOperationStatus::Unknown);
                assert!(unresolved.transaction_id.is_none());
            }
            _ => panic!("expected an unresolved recovery action"),
        }
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    #[tokio::test]
    async fn test_record_gate_waits_for_core_persistence_reply(#[case] accepted: bool) {
        use nautilus_common::messages::ExecutionEvent;
        use nautilus_model::{enums::AccountType, identifiers::TraderId, types::Currency};

        let operation_id = UUID4::new();
        let record = PositionOperationRecord::prepared(
            &ManagedOperationIntent {
                operation_id,
                trader_id: TraderId::from("TRADER-001"),
                strategy_id: StrategyId::from("STRATEGY-001"),
                account_id: AccountId::from("POLYMARKET-001"),
                operation: PositionOperationType::Convert,
                instrument_ids: vec![InstrumentId::from("1001.POLYMARKET")],
                barrier_instrument_ids: vec![InstrumentId::from("1001.POLYMARKET")],
                quantity: Some(Quantity::from("100")),
                condition_id: None,
                market_id: Some("0xmarket".into()),
                index_set: Some("1".into()),
            },
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            nautilus_core::UnixNanos::from(1),
        );
        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            TraderId::from("TRADER-001"),
            AccountId::from("POLYMARKET-001"),
            AccountType::Cash,
            Some(Currency::pUSD()),
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        let gate = CorePositionOperationRecordGate { emitter };

        let reply =
            async {
                let event = rx.recv().await.expect("expected a record control request");
                let ExecutionEvent::PositionOperationControl(
                    PositionOperationControlRequest::Record { key, reply, .. },
                ) = event
                else {
                    panic!("expected a record control request");
                };
                assert_eq!(
                    key,
                    PositionOperationRecord::key(&operation_id),
                    "record key must identify the operation"
                );
                if accepted {
                    reply(Ok(()));
                } else {
                    reply(Err("cache rejected".to_string()));
                }
            };
        let ((), result) = tokio::join!(reply, gate.confirm_prepared(&record));

        assert_eq!(result.is_ok(), accepted);
        if !accepted {
            assert_eq!(result.unwrap_err().to_string(), "cache rejected");
        }
    }

    #[rstest]
    fn test_split_roles_require_yes_and_no() {
        let outcomes = vec![
            metadata("0xc", "Yes", "10", None),
            metadata("0xc", "No", "11", None),
        ];
        assert!(ensure_yes_no_roles(&outcomes).is_ok());

        let same = vec![
            metadata("0xc", "Yes", "10", None),
            metadata("0xc", "Yes", "11", None),
        ];
        assert!(ensure_yes_no_roles(&same).is_err());

        let cross = vec![
            metadata("0xc", "Yes", "10", None),
            metadata("0xd", "No", "11", None),
        ];
        assert!(ensure_yes_no_roles(&cross).is_err());
    }

    #[rstest]
    fn test_convert_scope_allows_distinct_conditions_in_one_market() {
        let conditions = vec![
            metadata("0xc0", "No", "10", Some("0xmarket")),
            metadata("0xc1", "No", "11", Some("0xmarket")),
        ];
        assert!(validate_operation_scope(PositionOperationType::Convert, &conditions).is_ok());
        assert!(validate_operation_scope(PositionOperationType::Split, &conditions).is_err());

        let mixed = vec![
            metadata("0xc0", "No", "10", Some("0xmarket")),
            metadata("0xc1", "No", "11", Some("0xother")),
        ];
        assert!(validate_operation_scope(PositionOperationType::Convert, &mixed).is_err());
    }

    #[rstest]
    fn test_convert_requires_no_instruments() {
        let no = vec![metadata("0xc", "No", "10", Some("0xmarket"))];
        assert!(ensure_no_roles(&no).is_ok());

        let yes = vec![metadata("0xc", "Yes", "10", Some("0xmarket"))];
        assert!(ensure_no_roles(&yes).is_err());
    }

    #[rstest]
    fn test_common_market_id_requires_agreement() {
        let same = vec![
            metadata("0xc", "No", "10", Some("0xmarket")),
            metadata("0xd", "No", "11", Some("0xmarket")),
        ];
        assert_eq!(common_market_id(&same).unwrap(), "0xmarket");

        let missing = vec![metadata("0xc", "No", "10", None)];
        assert!(common_market_id(&missing).is_err());

        let different = vec![
            metadata("0xc", "No", "10", Some("0xmarket")),
            metadata("0xd", "No", "11", Some("0xother")),
        ];
        assert!(common_market_id(&different).is_err());
    }

    #[rstest]
    #[case("256", 0)]
    #[case("257", 1)]
    fn test_outcome_index_reads_final_byte(#[case] token: &str, #[case] expected: u8) {
        assert_eq!(outcome_index(token).unwrap(), expected);
    }

    #[rstest]
    #[case("258")]
    #[case("not-a-token")]
    fn test_outcome_index_rejects_invalid_tokens(#[case] token: &str) {
        assert!(outcome_index(token).is_err());
    }

    #[rstest]
    fn test_effect_summary_converts_base_units() {
        let effects = PositionOperationEffects {
            transaction_hash: alloy_primitives::B256::ZERO,
            block_number: 1,
            received: vec![TokenAmount {
                token_id: U256::from(10u8),
                quantity: U256::from(1_500_000u64),
            }],
            spent: vec![TokenAmount {
                token_id: U256::from(11u8),
                quantity: U256::from(1_000_000u64),
            }],
            collateral_delta: I256::try_from(500_000u64).unwrap(),
        };

        let summary = effect_summary(effects);
        assert_eq!(summary.received[0].token_id, "10");
        assert_eq!(summary.received[0].quantity, "1.5");
        assert_eq!(summary.spent[0].quantity, "1");
        assert_eq!(summary.collateral_delta, "0.5");
    }
}

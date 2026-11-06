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

//! Durable records for managed position operations.
//!
//! A record separates venue facts from local application facts. It is written before signing and
//! submission so that a crash cannot leave a signed request without a local trace. Cache persistence
//! is asynchronous: a successful write only means the configured persistence entry accepted it.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
    position_operation::{PositionOperationDirection, PositionOperationType},
    types::Quantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Current schema version for [`PositionOperationRecord`].
pub const POSITION_OPERATION_RECORD_SCHEMA_VERSION: u32 = 1;

/// Key prefix for operation records stored through Cache general bytes.
pub const POSITION_OPERATION_RECORD_PREFIX: &str = "position-operation";

/// One unsigned Deposit Wallet call captured before submission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionUnsignedCall {
    /// Target contract address as `0x`-prefixed hex.
    pub target: String,
    /// Call value in wei, as a decimal string.
    pub value: String,
    /// ABI-encoded calldata as `0x`-prefixed hex.
    pub data: String,
}

/// Venue-side lifecycle of the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionOperationVenueStatus {
    /// The request was prepared locally but not yet known to be sent.
    Prepared,
    /// The Relayer accepted the request and returned a transaction identifier.
    Submitted,
    /// The Relayer reported the transaction as confirmed.
    Confirmed,
    /// The Relayer reported failure.
    Failed,
    /// The Relayer reported the transaction as invalid.
    Invalid,
    /// The submission or effect outcome is unknown.
    Unknown,
}

/// Local application lifecycle of the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionOperationApplicationStatus {
    /// The operation has not been applied locally.
    Pending,
    /// All legs were applied locally.
    Applied,
    /// The request was rejected before any local application.
    Rejected,
    /// Evidence is missing or conflicting, so the local state remains unresolved.
    Unresolved,
}

/// One position-level allocation recorded after application.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PositionOperationAppliedLeg {
    /// The position affected by the leg.
    pub position_id: PositionId,
    /// The instrument affected by the leg.
    pub instrument_id: InstrumentId,
    /// The strategy which owns the position leg, when known.
    #[serde(default)]
    pub strategy_id: Option<StrategyId>,
    /// Whether the leg opened or closed quantity.
    pub direction: PositionOperationDirection,
    /// The operated quantity.
    pub quantity: Quantity,
    /// The exact cost basis added or released.
    pub cost: Decimal,
    /// The settlement-currency proceeds allocated to a close leg.
    pub income: Option<Decimal>,
    /// Whether the leg records physical payout of an already economically settled position.
    #[serde(default)]
    pub settled: bool,
}

/// A durable managed position operation record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PositionOperationRecord {
    /// Record schema version.
    pub schema_version: u32,
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The initiating trader.
    pub trader_id: TraderId,
    /// The initiating strategy.
    pub strategy_id: StrategyId,
    /// The account affected by the operation.
    pub account_id: AccountId,
    /// The Deposit Wallet address as `0x`-prefixed hex.
    pub wallet: String,
    /// The operation type.
    pub operation: PositionOperationType,
    /// The known instruments involved.
    pub instrument_ids: Vec<InstrumentId>,
    /// The complete account-level barrier scope resolved during preparation.
    pub barrier_instrument_ids: Vec<InstrumentId>,
    /// The operation quantity, when defined.
    pub quantity: Option<Quantity>,
    /// The CLOB condition identifier when applicable.
    pub condition_id: Option<String>,
    /// The on-chain negative-risk market identifier when applicable.
    pub market_id: Option<String>,
    /// The selected question indices as a decimal bitmask when applicable.
    pub index_set: Option<String>,
    /// The unsigned Deposit Wallet nonce as a decimal string.
    pub unsigned_nonce: Option<String>,
    /// The unsigned Deposit Wallet deadline as a decimal string.
    pub unsigned_deadline: Option<String>,
    /// The unsigned calls submitted in order.
    pub unsigned_calls: Vec<PositionUnsignedCall>,
    /// The Relayer transaction identifier, when known.
    pub venue_transaction_id: Option<String>,
    /// The on-chain transaction hash, when known.
    pub transaction_hash: Option<String>,
    /// The current venue lifecycle status.
    pub venue_status: PositionOperationVenueStatus,
    /// The current local application status.
    pub application_status: PositionOperationApplicationStatus,
    /// The per-leg allocations applied locally.
    pub applied_legs: Vec<PositionOperationAppliedLeg>,
    /// A human-readable detail or error message.
    pub message: Option<String>,
    /// UNIX timestamp (nanoseconds) when the record was created.
    pub created_at: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the record was last updated.
    pub updated_at: UnixNanos,
}

impl PositionOperationRecord {
    /// Returns the Cache general-bytes key for an operation record.
    #[must_use]
    pub fn key(operation_id: &UUID4) -> String {
        format!("{POSITION_OPERATION_RECORD_PREFIX}:{operation_id}")
    }

    /// Creates a prepared record before signing and submission.
    #[must_use]
    pub fn prepared(
        intent: &ManagedOperationIntent,
        wallet: &str,
        nonce: &str,
        deadline: &str,
        calls: Vec<PositionUnsignedCall>,
        ts: UnixNanos,
    ) -> Self {
        Self {
            schema_version: POSITION_OPERATION_RECORD_SCHEMA_VERSION,
            operation_id: intent.operation_id,
            trader_id: intent.trader_id,
            strategy_id: intent.strategy_id,
            account_id: intent.account_id,
            wallet: wallet.to_string(),
            operation: intent.operation,
            instrument_ids: intent.instrument_ids.clone(),
            barrier_instrument_ids: intent.barrier_instrument_ids.clone(),
            quantity: intent.quantity,
            condition_id: intent.condition_id.clone(),
            market_id: intent.market_id.clone(),
            index_set: intent.index_set.clone(),
            unsigned_nonce: Some(nonce.to_string()),
            unsigned_deadline: Some(deadline.to_string()),
            unsigned_calls: calls,
            venue_transaction_id: None,
            transaction_hash: None,
            venue_status: PositionOperationVenueStatus::Prepared,
            application_status: PositionOperationApplicationStatus::Pending,
            applied_legs: Vec::new(),
            message: None,
            created_at: ts,
            updated_at: ts,
        }
    }

    /// Marks the record submitted with a Relayer transaction identifier.
    pub fn mark_submitted(&mut self, transaction_id: String, ts: UnixNanos) {
        self.venue_transaction_id = Some(transaction_id);
        self.venue_status = PositionOperationVenueStatus::Submitted;
        self.updated_at = ts;
    }

    /// Marks the submission outcome unknown.
    pub fn mark_unknown(&mut self, message: String, ts: UnixNanos) {
        self.venue_status = PositionOperationVenueStatus::Unknown;
        self.message = Some(message);
        self.updated_at = ts;
    }

    /// Marks the submission explicitly rejected.
    pub fn mark_rejected(&mut self, message: String, ts: UnixNanos) {
        self.venue_status = PositionOperationVenueStatus::Failed;
        self.application_status = PositionOperationApplicationStatus::Rejected;
        self.message = Some(message);
        self.updated_at = ts;
    }

    /// Marks the local application complete.
    pub fn mark_applied(
        &mut self,
        legs: Vec<PositionOperationAppliedLeg>,
        transaction_hash: Option<String>,
        ts: UnixNanos,
    ) {
        self.venue_status = PositionOperationVenueStatus::Confirmed;
        self.application_status = PositionOperationApplicationStatus::Applied;
        self.applied_legs = legs;
        if transaction_hash.is_some() {
            self.transaction_hash = transaction_hash;
        }
        self.updated_at = ts;
    }

    /// Marks the venue outcome unresolved after recovery evidence could not be applied.
    pub fn mark_unresolved(
        &mut self,
        venue_status: PositionOperationVenueStatus,
        transaction_id: Option<String>,
        transaction_hash: Option<String>,
        message: String,
        ts: UnixNanos,
    ) {
        self.venue_status = venue_status;
        if transaction_id.is_some() {
            self.venue_transaction_id = transaction_id;
        }
        if transaction_hash.is_some() {
            self.transaction_hash = transaction_hash;
        }
        self.application_status = PositionOperationApplicationStatus::Unresolved;
        self.message = Some(message);
        self.updated_at = ts;
    }

    /// Returns whether startup recovery must treat the operation as still affecting the account.
    ///
    /// Applied or rejected operations and definitively failed or invalid submissions no longer
    /// block the account. Every other state requires evidence before new orders may proceed.
    #[must_use]
    pub fn requires_recovery(&self) -> bool {
        self.application_status != PositionOperationApplicationStatus::Rejected
            && !matches!(
                self.venue_status,
                PositionOperationVenueStatus::Failed | PositionOperationVenueStatus::Invalid
            )
    }
}

/// Context for a managed operation prepared by the execution client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedOperationIntent {
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The initiating trader.
    pub trader_id: TraderId,
    /// The initiating strategy.
    pub strategy_id: StrategyId,
    /// The account affected by the operation.
    pub account_id: AccountId,
    /// The operation type.
    pub operation: PositionOperationType,
    /// The known instruments involved.
    pub instrument_ids: Vec<InstrumentId>,
    /// The complete account-level barrier scope resolved during preparation.
    pub barrier_instrument_ids: Vec<InstrumentId>,
    /// The operation quantity, when defined.
    pub quantity: Option<Quantity>,
    /// The CLOB condition identifier when applicable.
    pub condition_id: Option<String>,
    /// The on-chain negative-risk market identifier when applicable.
    pub market_id: Option<String>,
    /// The selected question indices as a decimal bitmask when applicable.
    pub index_set: Option<String>,
}

/// Recorder for managed position operation submission facts.
///
/// Implementations must be thread safe because submission runs on an asynchronous task. Only
/// [`Self::on_prepared`] gates submission: an error aborts before signing and sending. Later
/// notifications are best effort; the adapter logs their errors without resubmitting.
pub trait PositionOperationRecorder: std::fmt::Debug + Send + Sync {
    /// Records the operation intent and unsigned submission before signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the record cannot be persisted. The caller must not submit.
    fn on_prepared(&self, record: &PositionOperationRecord) -> anyhow::Result<()>;

    /// Records the Relayer transaction identifier after a successful submit.
    fn on_submitted(&self, operation_id: UUID4, transaction_id: &str, ts: UnixNanos);

    /// Records a submission failure. `ambiguous` is true when the request may have reached the
    /// venue.
    fn on_submission_failed(
        &self,
        operation_id: UUID4,
        error: &str,
        ambiguous: bool,
        ts: UnixNanos,
    );

    /// Returns the currently known record, when the recorder retains it.
    fn load(&self, _operation_id: &UUID4) -> Option<PositionOperationRecord> {
        None
    }

    /// Records the applied position legs after the core commits the operation.
    fn on_applied(
        &self,
        _operation_id: UUID4,
        _legs: &[PositionOperationAppliedLeg],
        _transaction_hash: Option<&str>,
        _ts: UnixNanos,
    ) {
    }

    /// Records that recovery evidence could not be applied locally.
    fn on_unresolved(
        &self,
        _operation_id: UUID4,
        _venue_status: PositionOperationVenueStatus,
        _transaction_id: Option<&str>,
        _transaction_hash: Option<&str>,
        _message: &str,
        _ts: UnixNanos,
    ) {
    }
}

/// Loads every position operation record persisted in the Cache general store.
///
/// # Errors
///
/// Returns an error if a stored record is malformed or uses an unsupported schema version.
pub fn load_position_operation_records(
    cache: &nautilus_common::cache::Cache,
) -> anyhow::Result<Vec<PositionOperationRecord>> {
    let prefix = format!("{POSITION_OPERATION_RECORD_PREFIX}:");
    let mut records = Vec::new();

    for (key, bytes) in cache.general_entries_with_prefix(&prefix) {
        let record: PositionOperationRecord = serde_json::from_slice(&bytes)
            .map_err(|e| anyhow::anyhow!("invalid position operation record `{key}`: {e}"))?;

        if record.schema_version != POSITION_OPERATION_RECORD_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported position operation record `{key}` schema version: {}",
                record.schema_version
            );
        }
        records.push(record);
    }
    Ok(records)
}

/// Store abstraction for operation records backed by Cache general bytes and its persistence.
pub trait PositionOperationRecordStore: std::fmt::Debug + Send + Sync {
    /// Loads a record by operation identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the stored payload cannot be read or decoded.
    fn load(&self, operation_id: &UUID4) -> anyhow::Result<Option<PositionOperationRecord>>;

    /// Saves a record by operation identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload cannot be encoded or persisted.
    fn save(&self, record: &PositionOperationRecord) -> anyhow::Result<()>;

    /// Restores records loaded from persistent storage into the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot accept the recovered records.
    fn restore(&self, records: Vec<PositionOperationRecord>) -> anyhow::Result<()>;

    /// Returns every record retained by the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be read.
    fn records(&self) -> anyhow::Result<Vec<PositionOperationRecord>>;
}

/// Asynchronous gate that confirms the operation record before signing.
///
/// Live installs a gate that hands the record to the Cache-owning execution thread and awaits an
/// explicit persistence result. The submission task yields while waiting and must not sign or send
/// until the gate succeeds.
#[async_trait::async_trait]
pub trait PositionOperationRecordGate: std::fmt::Debug + Send + Sync {
    /// Confirms that the prepared record is durably accepted before signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the core thread rejects or fails to persist the record.
    async fn confirm_prepared(&self, record: &PositionOperationRecord) -> anyhow::Result<()>;
}

/// A recorder which persists records and updates their submission status.
#[derive(Debug)]
pub struct PositionOperationStoreRecorder {
    store: std::sync::Arc<dyn PositionOperationRecordStore>,
}

impl PositionOperationStoreRecorder {
    /// Creates a recorder over the given store.
    #[must_use]
    pub fn new(store: std::sync::Arc<dyn PositionOperationRecordStore>) -> Self {
        Self { store }
    }

    fn update(
        &self,
        operation_id: UUID4,
        ts: UnixNanos,
        update: impl FnOnce(&mut PositionOperationRecord),
    ) {
        match self.store.load(&operation_id) {
            Ok(Some(mut record)) => {
                update(&mut record);
                record.updated_at = ts;
                if let Err(e) = self.store.save(&record) {
                    log::error!("Failed to persist position operation {operation_id} status: {e}");
                }
            }
            Ok(None) => {
                log::error!(
                    "Position operation record {operation_id} is missing while updating status"
                );
            }
            Err(e) => {
                log::error!(
                    "Failed to load position operation {operation_id} while updating status: {e}"
                );
            }
        }
    }
}

impl PositionOperationRecorder for PositionOperationStoreRecorder {
    fn on_prepared(&self, record: &PositionOperationRecord) -> anyhow::Result<()> {
        self.store.save(record)
    }

    fn on_submitted(&self, operation_id: UUID4, transaction_id: &str, ts: UnixNanos) {
        self.update(operation_id, ts, |record| {
            record.mark_submitted(transaction_id.to_string(), ts);
        });
    }

    fn on_submission_failed(
        &self,
        operation_id: UUID4,
        error: &str,
        ambiguous: bool,
        ts: UnixNanos,
    ) {
        self.update(operation_id, ts, |record| {
            if ambiguous {
                record.mark_unknown(error.to_string(), ts);
            } else {
                record.mark_rejected(error.to_string(), ts);
            }
        });
    }

    fn load(&self, operation_id: &UUID4) -> Option<PositionOperationRecord> {
        self.store.load(operation_id).ok().flatten()
    }

    fn on_applied(
        &self,
        operation_id: UUID4,
        legs: &[PositionOperationAppliedLeg],
        transaction_hash: Option<&str>,
        ts: UnixNanos,
    ) {
        self.update(operation_id, ts, |record| {
            record.mark_applied(legs.to_vec(), transaction_hash.map(str::to_string), ts);
        });
    }

    fn on_unresolved(
        &self,
        operation_id: UUID4,
        venue_status: PositionOperationVenueStatus,
        transaction_id: Option<&str>,
        transaction_hash: Option<&str>,
        message: &str,
        ts: UnixNanos,
    ) {
        self.update(operation_id, ts, |record| {
            record.mark_unresolved(
                venue_status,
                transaction_id.map(str::to_string),
                transaction_hash.map(str::to_string),
                message.to_string(),
                ts,
            );
        });
    }
}

/// A thread-safe in-memory operation record store.
///
/// The adapter uses this store by default so an operation is always recorded before signing.
/// A Cache-backed store can replace it through [`PositionOperationStoreRecorder`] to persist
/// records through the configured Cache backend.
#[derive(Debug, Default)]
pub struct InMemoryPositionOperationStore {
    records: std::sync::Mutex<std::collections::HashMap<UUID4, PositionOperationRecord>>,
}

impl InMemoryPositionOperationStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl PositionOperationRecordStore for InMemoryPositionOperationStore {
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
        Ok(())
    }

    fn restore(&self, records: Vec<PositionOperationRecord>) -> anyhow::Result<()> {
        let mut stored = self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("position operation store lock poisoned"))?;
        for record in records {
            stored.insert(record.operation_id, record);
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use nautilus_model::position_operation::PositionOperationType;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[derive(Debug, Default)]
    struct MemoryStore {
        records: Mutex<HashMap<UUID4, PositionOperationRecord>>,
    }

    impl PositionOperationRecordStore for MemoryStore {
        fn load(&self, operation_id: &UUID4) -> anyhow::Result<Option<PositionOperationRecord>> {
            Ok(self.records.lock().unwrap().get(operation_id).cloned())
        }

        fn save(&self, record: &PositionOperationRecord) -> anyhow::Result<()> {
            self.records
                .lock()
                .unwrap()
                .insert(record.operation_id, record.clone());
            Ok(())
        }

        fn restore(&self, records: Vec<PositionOperationRecord>) -> anyhow::Result<()> {
            let mut stored = self.records.lock().unwrap();
            for record in records {
                stored.insert(record.operation_id, record);
            }
            Ok(())
        }

        fn records(&self) -> anyhow::Result<Vec<PositionOperationRecord>> {
            Ok(self.records.lock().unwrap().values().cloned().collect())
        }
    }

    fn intent(operation_id: UUID4) -> ManagedOperationIntent {
        ManagedOperationIntent {
            operation_id,
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            account_id: AccountId::from("POLYMARKET-001"),
            operation: PositionOperationType::Convert,
            instrument_ids: vec![InstrumentId::from("TOKEN.POLYMARKET")],
            barrier_instrument_ids: vec![InstrumentId::from("TOKEN.POLYMARKET")],
            quantity: Some(Quantity::from("100")),
            condition_id: None,
            market_id: Some("0xmarket".into()),
            index_set: Some("3".into()),
        }
    }

    #[rstest]
    fn test_record_key_is_stable() {
        let operation_id = UUID4::from("00000000-0000-4000-8000-000000000001");
        assert_eq!(
            PositionOperationRecord::key(&operation_id),
            "position-operation:00000000-0000-4000-8000-000000000001"
        );
    }

    #[rstest]
    fn test_prepared_record_round_trips() {
        let operation_id = UUID4::new();
        let record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            vec![PositionUnsignedCall {
                target: "0xadA2005600Dec949baf300f4C6120000bDB6eAab".into(),
                value: "0".into(),
                data: "0xc64748c4".into(),
            }],
            UnixNanos::from(1),
        );

        let json = serde_json::to_string(&record).unwrap();
        let restored: PositionOperationRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, record);
        assert_eq!(
            restored.schema_version,
            POSITION_OPERATION_RECORD_SCHEMA_VERSION
        );
        assert_eq!(
            restored.venue_status,
            PositionOperationVenueStatus::Prepared
        );
        assert_eq!(
            restored.application_status,
            PositionOperationApplicationStatus::Pending
        );
    }

    #[rstest]
    fn test_load_position_operation_records_from_cache() {
        let operation_id = UUID4::new();
        let record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );

        let mut cache = nautilus_common::cache::Cache::default();
        cache
            .add(
                &PositionOperationRecord::key(&operation_id),
                serde_json::to_vec(&record).unwrap().into(),
            )
            .unwrap();

        assert_eq!(
            load_position_operation_records(&cache).unwrap(),
            vec![record]
        );
    }

    #[rstest]
    fn test_load_position_operation_records_rejects_unknown_schema() {
        let operation_id = UUID4::new();
        let mut record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );
        record.schema_version = POSITION_OPERATION_RECORD_SCHEMA_VERSION + 1;

        let mut cache = nautilus_common::cache::Cache::default();
        cache
            .add(
                &PositionOperationRecord::key(&operation_id),
                serde_json::to_vec(&record).unwrap().into(),
            )
            .unwrap();

        let error = load_position_operation_records(&cache).unwrap_err();
        assert!(error.to_string().contains("unsupported"));
    }

    #[rstest]
    fn test_requires_recovery_follows_venue_and_application_status() {
        let operation_id = UUID4::new();
        let mut record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );

        assert!(record.requires_recovery());

        record.venue_status = PositionOperationVenueStatus::Failed;
        assert!(!record.requires_recovery());

        record.venue_status = PositionOperationVenueStatus::Confirmed;
        record.application_status = PositionOperationApplicationStatus::Applied;
        assert!(record.requires_recovery());

        record.application_status = PositionOperationApplicationStatus::Unresolved;
        assert!(record.requires_recovery());
    }

    #[rstest]
    fn test_legacy_applied_leg_json_defaults_new_fields() {
        let json = r#"{
            "position_id": "P-001",
            "instrument_id": "TOKEN.POLYMARKET",
            "direction": "Open",
            "quantity": "100",
            "cost": "50",
            "income": null
        }"#;

        let leg: PositionOperationAppliedLeg = serde_json::from_str(json).unwrap();
        assert!(leg.strategy_id.is_none());
        assert!(!leg.settled);
    }

    #[rstest]
    fn test_record_mark_unresolved_remains_recoverable() {
        let operation_id = UUID4::new();
        let mut record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );

        record.mark_unresolved(
            PositionOperationVenueStatus::Confirmed,
            Some("tx-1".to_string()),
            Some("0xhash".to_string()),
            "effects are incomplete".to_string(),
            UnixNanos::from(2),
        );

        assert_eq!(record.venue_status, PositionOperationVenueStatus::Confirmed);
        assert_eq!(
            record.application_status,
            PositionOperationApplicationStatus::Unresolved
        );
        assert_eq!(record.venue_transaction_id.as_deref(), Some("tx-1"));
        assert_eq!(record.transaction_hash.as_deref(), Some("0xhash"));
        assert!(record.requires_recovery());
    }

    #[rstest]
    fn test_store_returns_every_retained_record() {
        let store = InMemoryPositionOperationStore::new();
        let first = UUID4::new();
        let second = UUID4::new();
        let first_record = PositionOperationRecord::prepared(
            &intent(first),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );
        let second_record = PositionOperationRecord::prepared(
            &intent(second),
            "0x1111111111111111111111111111111111111111",
            "8",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );

        store.save(&first_record).unwrap();
        store.save(&second_record).unwrap();

        let mut records = store.records().unwrap();
        records.sort_by_key(|record| record.operation_id.to_string());
        assert_eq!(records.len(), 2);
    }

    #[rstest]
    fn test_store_recorder_updates_submission_status() {
        let store = Arc::new(MemoryStore::default());
        let recorder = PositionOperationStoreRecorder::new(store.clone());
        let operation_id = UUID4::new();
        let mut record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );
        recorder.on_prepared(&record).unwrap();
        record.mark_submitted("tx-1".into(), UnixNanos::from(2));
        recorder.on_submitted(operation_id, "tx-1", UnixNanos::from(2));

        let stored = store.load(&operation_id).unwrap().unwrap();
        assert_eq!(stored.venue_transaction_id.as_deref(), Some("tx-1"));
        assert_eq!(stored.venue_status, PositionOperationVenueStatus::Submitted);
    }

    #[rstest]
    fn test_store_recorder_marks_ambiguous_and_rejected_failures() {
        let store = Arc::new(MemoryStore::default());
        let recorder = PositionOperationStoreRecorder::new(store.clone());
        let operation_id = UUID4::new();
        recorder
            .on_prepared(&PositionOperationRecord::prepared(
                &intent(operation_id),
                "0x1111111111111111111111111111111111111111",
                "7",
                "1700000000",
                Vec::new(),
                UnixNanos::from(1),
            ))
            .unwrap();

        recorder.on_submission_failed(operation_id, "timeout", true, UnixNanos::from(2));
        let stored = store.load(&operation_id).unwrap().unwrap();
        assert_eq!(stored.venue_status, PositionOperationVenueStatus::Unknown);
        assert_eq!(
            stored.application_status,
            PositionOperationApplicationStatus::Pending
        );

        recorder.on_submission_failed(operation_id, "rejected", false, UnixNanos::from(3));
        let stored = store.load(&operation_id).unwrap().unwrap();
        assert_eq!(stored.venue_status, PositionOperationVenueStatus::Failed);
        assert_eq!(
            stored.application_status,
            PositionOperationApplicationStatus::Rejected
        );
    }

    #[rstest]
    fn test_store_recorder_marks_applied() {
        let store = Arc::new(MemoryStore::default());
        let recorder = PositionOperationStoreRecorder::new(store.clone());
        let operation_id = UUID4::new();
        recorder
            .on_prepared(&PositionOperationRecord::prepared(
                &intent(operation_id),
                "0x1111111111111111111111111111111111111111",
                "7",
                "1700000000",
                Vec::new(),
                UnixNanos::from(1),
            ))
            .unwrap();

        recorder.on_applied(
            operation_id,
            &[PositionOperationAppliedLeg {
                position_id: PositionId::from("P-001"),
                instrument_id: InstrumentId::from("TOKEN.POLYMARKET"),
                strategy_id: None,
                direction: PositionOperationDirection::Open,
                quantity: Quantity::from("100"),
                cost: dec!(50),
                income: None,
                settled: false,
            }],
            Some("0xabc"),
            UnixNanos::from(2),
        );

        let stored = store.load(&operation_id).unwrap().unwrap();
        assert_eq!(
            stored.application_status,
            PositionOperationApplicationStatus::Applied
        );
        assert_eq!(stored.applied_legs.len(), 1);
        assert_eq!(stored.transaction_hash.as_deref(), Some("0xabc"));
    }

    #[rstest]
    fn test_applied_record_carries_legs() {
        let operation_id = UUID4::new();
        let mut record = PositionOperationRecord::prepared(
            &intent(operation_id),
            "0x1111111111111111111111111111111111111111",
            "7",
            "1700000000",
            Vec::new(),
            UnixNanos::from(1),
        );
        record.mark_applied(
            vec![PositionOperationAppliedLeg {
                position_id: PositionId::from("P-001"),
                instrument_id: InstrumentId::from("TOKEN.POLYMARKET"),
                strategy_id: None,
                direction: PositionOperationDirection::Open,
                quantity: Quantity::from("100"),
                cost: dec!(50),
                income: None,
                settled: false,
            }],
            Some("0xabc".into()),
            UnixNanos::from(4),
        );

        assert_eq!(record.applied_legs.len(), 1);
        assert_eq!(record.transaction_hash.as_deref(), Some("0xabc"));
        assert_eq!(
            record.application_status,
            PositionOperationApplicationStatus::Applied
        );
    }
}

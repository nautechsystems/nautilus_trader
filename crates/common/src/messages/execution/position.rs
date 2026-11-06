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

//! Managed position operation commands and results.

use std::{fmt::Display, sync::Arc};

use ahash::AHashSet;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, PositionId, StrategyId, TraderId},
    position_operation::{PositionOperationDirection, PositionOperationType},
    types::Quantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Reply callback used by an internal position operation control request.
pub type PositionOperationControlReply = Arc<dyn Fn(Result<(), String>) + Send + Sync>;

/// A position leg recorded for a managed operation before a restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionOperationRecoveryLeg {
    /// The affected position.
    pub position_id: PositionId,
    /// The affected instrument.
    pub instrument_id: InstrumentId,
    /// The strategy which owns the position leg, when recorded.
    pub strategy_id: Option<StrategyId>,
    /// Whether the leg opened or closed quantity.
    pub direction: PositionOperationDirection,
    /// The operated quantity.
    pub quantity: Quantity,
    /// The exact cost basis added or released.
    pub cost: Decimal,
    /// The exact settlement-currency proceeds allocated to a close leg.
    pub income: Option<Decimal>,
    /// Whether the leg records physical payout of an already economically settled position.
    pub settled: bool,
}

/// A request to reconcile a durable operation record with persisted position replay state.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionOperationRecovery {
    /// The initiating trader.
    pub trader_id: TraderId,
    /// The initiating strategy.
    pub strategy_id: StrategyId,
    /// The execution client which processed the operation.
    pub client_id: ClientId,
    /// The affected account.
    pub account_id: AccountId,
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The operation type.
    pub operation: PositionOperationType,
    /// The complete account-level barrier scope.
    pub instrument_ids: Vec<InstrumentId>,
    /// The operation quantity, when defined.
    pub quantity: Option<Quantity>,
    /// Verified venue effects, when available.
    pub effects: Option<PositionOperationEffects>,
    /// The Relayer transaction identifier, when known.
    pub venue_transaction_id: Option<String>,
    /// The on-chain transaction hash, when known.
    pub transaction_hash: Option<String>,
    /// Legs already allocated by the original application, when recorded.
    pub applied_legs: Vec<PositionOperationRecoveryLeg>,
}

/// Result of reconciling a recovered operation with position replay state.
#[derive(Clone, Debug, PartialEq)]
pub enum PositionOperationRecoveryOutcome {
    /// Every leg is present and consistent, or was safely completed.
    Applied {
        /// The complete final leg set.
        legs: Vec<PositionOperationRecoveryLeg>,
    },
    /// Evidence is missing or conflicting; the account remains blocked.
    Unresolved {
        /// Human-readable reason.
        message: String,
    },
}

/// Reply callback for a position operation recovery request.
pub type PositionOperationRecoveryReply =
    Arc<dyn Fn(Result<PositionOperationRecoveryOutcome, String>) + Send + Sync>;

/// An unresolved startup recovery outcome kept blocking the affected account scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionOperationRecoveryUnresolved {
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The latest known venue status.
    pub venue_status: PositionOperationStatus,
    /// The Relayer transaction identifier, when known.
    pub transaction_id: Option<String>,
    /// The on-chain transaction hash, when known.
    pub transaction_hash: Option<String>,
    /// Human-readable reason.
    pub message: String,
}

/// A terminal startup recovery failure that no longer blocks the account scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionOperationRecoveryFailed {
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The complete account-level barrier scope.
    pub instrument_ids: Vec<InstrumentId>,
    /// The Relayer transaction identifier, when known.
    pub transaction_id: Option<String>,
    /// The on-chain transaction hash, when known.
    pub transaction_hash: Option<String>,
    /// Human-readable reason.
    pub message: String,
}

/// One prepared startup recovery action for the execution engine.
#[derive(Clone, Debug, PartialEq)]
pub enum PositionOperationStartupRecovery {
    /// Evidence is available; reconcile it against position replay state.
    Reconcile(Box<PositionOperationRecovery>),
    /// Evidence is insufficient; keep blocking without re-signing or resubmitting.
    Unresolved(Box<PositionOperationRecoveryUnresolved>),
    /// The venue reported a definitive terminal failure.
    Failed(Box<PositionOperationRecoveryFailed>),
}

/// Internal adapter-to-core control requests for managed position operations.
///
/// These requests are exchanged over the live event channel so the core thread performs barrier
/// scope checks and Cache record writes. The adapter awaits the reply before signing.
pub enum PositionOperationControlRequest {
    /// Requests an atomic barrier over the complete input/output instrument scope.
    Barrier {
        /// The stable operation identity.
        operation_id: UUID4,
        /// The operation type, which determines whether settled instruments may be redeemed.
        operation: PositionOperationType,
        /// The complete account-level scope.
        instrument_ids: Vec<InstrumentId>,
        /// The strategy whose book positions supply the operation inputs.
        strategy_id: StrategyId,
        /// The account whose book positions supply the operation inputs.
        account_id: AccountId,
        /// The operation quantity; absent only for a full CTF redeem.
        quantity: Option<Quantity>,
        /// The operation inputs whose local ownership must be verified under the barrier.
        input_instrument_ids: Vec<InstrumentId>,
        /// Reply with success only after the full barrier is installed.
        reply: PositionOperationControlReply,
    },
    /// Requests a synchronously confirmed Cache record write before signing.
    Record {
        /// Cache general-bytes key.
        key: String,
        /// Serialized operation record.
        record: serde_json::Value,
        /// Reply with success only after the core accepted the record.
        reply: PositionOperationControlReply,
    },
    /// Reconciled restart recovery against persisted position replay state.
    Recover {
        /// The recovered operation evidence and recorded legs.
        recovery: Box<PositionOperationRecovery>,
        /// Reply with the reconciled outcome.
        reply: PositionOperationRecoveryReply,
    },
}

impl std::fmt::Debug for PositionOperationControlRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Barrier {
                operation_id,
                operation,
                instrument_ids,
                ..
            } => f
                .debug_struct("Barrier")
                .field("operation_id", operation_id)
                .field("operation", operation)
                .field("instrument_ids", instrument_ids)
                .finish(),
            Self::Record { key, .. } => f.debug_struct("Record").field("key", key).finish(),
            Self::Recover { recovery, .. } => f
                .debug_struct("Recover")
                .field("operation_id", &recovery.operation_id)
                .field("operation", &recovery.operation)
                .finish(),
        }
    }
}

impl Display for PositionOperationControlRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Barrier {
                operation_id,
                instrument_ids,
                ..
            } => write!(
                f,
                "PositionOperationControlRequest::Barrier(operation_id={operation_id}, instruments={})",
                instrument_ids.len()
            ),
            Self::Record { key, .. } => {
                write!(f, "PositionOperationControlRequest::Record(key={key})")
            }
            Self::Recover { recovery, .. } => write!(
                f,
                "PositionOperationControlRequest::Recover(operation_id={}, operation={})",
                recovery.operation_id, recovery.operation
            ),
        }
    }
}

/// A command requesting a managed position operation.
///
/// The command UUID is the stable operation identity; there is no second request identity.
/// `instrument_ids` roles are validated per operation: split and merge take the two outcomes of
/// one condition, CTF redeem takes both outcomes, V2 redeem takes one outcome, and convert takes
/// one or more NO outcomes of one negative-risk event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct SubmitPositionOperation {
    /// The trader ID associated with the command.
    pub trader_id: TraderId,
    /// The execution client which should process the operation.
    pub client_id: Option<ClientId>,
    /// The strategy initiating the operation.
    pub strategy_id: StrategyId,
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The operation type.
    pub operation: PositionOperationType,
    /// The known instruments involved in the operation.
    pub instrument_ids: Vec<InstrumentId>,
    /// The operation quantity, when the operation defines one.
    pub quantity: Option<Quantity>,
    /// UNIX timestamp (nanoseconds) when the command was initialized.
    pub ts_init: UnixNanos,
}

impl SubmitPositionOperation {
    /// Creates a new [`SubmitPositionOperation`] instance.
    #[expect(
        clippy::too_many_arguments,
        reason = "command constructor mirrors the routed request fields"
    )]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        operation_id: UUID4,
        operation: PositionOperationType,
        instrument_ids: Vec<InstrumentId>,
        quantity: Option<Quantity>,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id,
            operation_id,
            operation,
            instrument_ids,
            quantity,
            ts_init,
        }
    }

    /// Validates instrument roles, uniqueness, and quantity rules for the operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the instruments are empty or duplicated, their count does not match
    /// the operation, or the quantity presence or sign is invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.instrument_ids.is_empty(),
            "position operation requires at least one instrument"
        );

        let mut seen = AHashSet::new();
        for instrument_id in &self.instrument_ids {
            anyhow::ensure!(
                seen.insert(*instrument_id),
                "position operation repeats instrument {instrument_id}"
            );
        }

        match self.operation {
            PositionOperationType::Split | PositionOperationType::Merge => {
                anyhow::ensure!(
                    self.instrument_ids.len() == 2,
                    "{} requires both outcome instruments",
                    self.operation
                );
                Self::require_positive_quantity(self.quantity, self.operation)?;
            }
            PositionOperationType::Redeem => match self.instrument_ids.len() {
                2 => anyhow::ensure!(
                    self.quantity.is_none(),
                    "CTF redeem does not take a quantity"
                ),
                1 => Self::require_positive_quantity(self.quantity, self.operation)?,
                count => anyhow::bail!(
                    "redeem requires one outcome instrument or both outcomes of one condition, was {count}"
                ),
            },
            PositionOperationType::Convert => {
                Self::require_positive_quantity(self.quantity, self.operation)?;
            }
        }

        Ok(())
    }

    fn require_positive_quantity(
        quantity: Option<Quantity>,
        operation: PositionOperationType,
    ) -> anyhow::Result<()> {
        match quantity {
            Some(quantity) if !quantity.is_zero() => Ok(()),
            Some(_) => anyhow::bail!("{operation} quantity must be positive"),
            None => anyhow::bail!("{operation} requires a quantity"),
        }
    }
}

impl Display for SubmitPositionOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SubmitPositionOperation(operation_id={}, operation={}, strategy_id={}, client_id={:?}, instruments={:?}, quantity={:?})",
            self.operation_id,
            self.operation,
            self.strategy_id,
            self.client_id,
            self.instrument_ids,
            self.quantity,
        )
    }
}

/// A query for the known state of a position operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct QueryPositionOperation {
    /// The trader ID associated with the query.
    pub trader_id: TraderId,
    /// The execution client which should answer the query.
    pub client_id: Option<ClientId>,
    /// The strategy requesting the query.
    pub strategy_id: StrategyId,
    /// The operation identity to look up.
    pub operation_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the query was initialized.
    pub ts_init: UnixNanos,
}

impl QueryPositionOperation {
    /// Creates a new [`QueryPositionOperation`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        operation_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id,
            operation_id,
            ts_init,
        }
    }
}

impl Display for QueryPositionOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "QueryPositionOperation(operation_id={}, strategy_id={}, client_id={:?})",
            self.operation_id, self.strategy_id, self.client_id,
        )
    }
}

/// The lifecycle status reported for a position operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionOperationStatus {
    /// The operation was accepted for submission.
    Submitted,
    /// The venue reported its transaction as confirmed.
    Confirmed,
    /// The core applied the operation to positions, cache, and portfolio.
    Applied,
    /// The request was rejected before submission.
    Rejected,
    /// The venue reported failure or the signed transaction is invalid.
    Failed,
    /// The submission outcome or on-chain effect is not yet known.
    Unknown,
}

impl PositionOperationStatus {
    /// Returns the lowercase status name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Confirmed => "confirmed",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

impl Display for PositionOperationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One token amount in a normalized on-chain effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionOperationTokenAmount {
    /// Conditional Tokens position identifier.
    pub token_id: String,
    /// Token quantity in six-decimal base units as an exact decimal string.
    pub quantity: String,
}

/// One verified CTF payout numerator for a spent token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionOperationTokenPayout {
    /// Conditional Tokens position identifier.
    pub token_id: String,
    /// Payout numerator for the condition outcome.
    pub numerator: String,
    /// Payout denominator for the condition.
    pub denominator: String,
}

/// Normalized on-chain effects of a confirmed position operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionOperationEffects {
    /// Outcome tokens received by the wallet.
    pub received: Vec<PositionOperationTokenAmount>,
    /// Outcome tokens spent by the wallet.
    pub spent: Vec<PositionOperationTokenAmount>,
    /// Net pUSD collateral change, positive for inflows, as an exact decimal string.
    pub collateral_delta: String,
    /// Verified CTF payout fractions for spent tokens, when applicable.
    #[serde(default)]
    pub payouts: Vec<PositionOperationTokenPayout>,
}

/// A result carrying the known state of a position operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct PositionOperationResult {
    /// The trader ID associated with the operation.
    pub trader_id: TraderId,
    /// The strategy which initiated the operation.
    pub strategy_id: StrategyId,
    /// The execution client which processed the operation.
    pub client_id: ClientId,
    /// The account affected by the operation.
    pub account_id: AccountId,
    /// The stable operation identity.
    pub operation_id: UUID4,
    /// The operation type.
    pub operation: PositionOperationType,
    /// The known operation status.
    pub status: PositionOperationStatus,
    /// The instruments involved in the operation.
    pub instrument_ids: Vec<InstrumentId>,
    /// The Relayer transaction identifier, when known.
    pub venue_transaction_id: Option<String>,
    /// The on-chain transaction hash, when known.
    pub transaction_hash: Option<String>,
    /// A human-readable detail or error message.
    pub message: Option<String>,
    /// Normalized on-chain effects when the transaction is confirmed and parsed.
    pub effects: Option<PositionOperationEffects>,
    /// The unique identifier for the result event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the result occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the result was initialized.
    pub ts_init: UnixNanos,
}

impl PositionOperationResult {
    /// Creates a new [`PositionOperationResult`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        client_id: ClientId,
        account_id: AccountId,
        operation_id: UUID4,
        operation: PositionOperationType,
        status: PositionOperationStatus,
        instrument_ids: Vec<InstrumentId>,
        venue_transaction_id: Option<String>,
        transaction_hash: Option<String>,
        message: Option<String>,
        effects: Option<PositionOperationEffects>,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            client_id,
            account_id,
            operation_id,
            operation,
            status,
            instrument_ids,
            venue_transaction_id,
            transaction_hash,
            message,
            effects,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for PositionOperationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PositionOperationResult(operation_id={}, operation={}, status={}, client_id={}, account_id={})",
            self.operation_id, self.operation, self.status, self.client_id, self.account_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;

    fn command(
        operation: PositionOperationType,
        instruments: Vec<InstrumentId>,
    ) -> SubmitPositionOperation {
        SubmitPositionOperation::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("POLYMARKET")),
            StrategyId::from("STRATEGY-001"),
            UUID4::new(),
            operation,
            instruments,
            Some(Quantity::from("100")),
            UnixNanos::from(1),
        )
    }

    fn condition_outcomes() -> Vec<InstrumentId> {
        vec![
            InstrumentId::from("YES-TOKEN.POLYMARKET"),
            InstrumentId::from("NO-TOKEN.POLYMARKET"),
        ]
    }

    #[rstest]
    fn test_split_and_merge_require_both_outcomes() {
        assert!(
            command(PositionOperationType::Split, condition_outcomes())
                .validate()
                .is_ok()
        );
        assert!(
            command(PositionOperationType::Merge, condition_outcomes())
                .validate()
                .is_ok()
        );
        assert!(
            command(
                PositionOperationType::Split,
                vec![InstrumentId::from("YES-TOKEN.POLYMARKET")]
            )
            .validate()
            .is_err()
        );
    }

    #[rstest]
    fn test_ctf_and_v2_redeem_quantity_rules() {
        let mut ctf = command(PositionOperationType::Redeem, condition_outcomes());
        assert!(ctf.validate().is_err());
        ctf.quantity = None;
        assert!(ctf.validate().is_ok());

        let v2 = command(
            PositionOperationType::Redeem,
            vec![InstrumentId::from("YES-TOKEN.POLYMARKET")],
        );
        assert!(v2.validate().is_ok());
    }

    #[rstest]
    fn test_convert_accepts_multiple_no_outcomes() {
        let convert = command(
            PositionOperationType::Convert,
            vec![
                InstrumentId::from("NO-A.POLYMARKET"),
                InstrumentId::from("NO-B.POLYMARKET"),
            ],
        );
        assert!(convert.validate().is_ok());
    }

    #[rstest]
    fn test_rejects_duplicates_and_empty_quantity() {
        let duplicated = command(
            PositionOperationType::Split,
            vec![
                InstrumentId::from("YES-TOKEN.POLYMARKET"),
                InstrumentId::from("YES-TOKEN.POLYMARKET"),
            ],
        );
        assert!(duplicated.validate().is_err());

        let mut zero = command(PositionOperationType::Split, condition_outcomes());
        zero.quantity = Some(Quantity::from("0"));
        assert!(zero.validate().is_err());

        let mut missing = command(PositionOperationType::Split, condition_outcomes());
        missing.quantity = None;
        assert!(missing.validate().is_err());
    }

    #[rstest]
    fn test_result_serialization_round_trip() {
        let result = PositionOperationResult::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRATEGY-001"),
            ClientId::from("POLYMARKET"),
            AccountId::from("POLYMARKET-001"),
            UUID4::new(),
            PositionOperationType::Convert,
            PositionOperationStatus::Submitted,
            condition_outcomes(),
            Some("tx-1".into()),
            Some("0xabc".into()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        let json = serde_json::to_string(&result).unwrap();
        let restored: PositionOperationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, result);
        assert_eq!(restored.status.to_string(), "submitted");
    }

    #[rstest]
    fn test_command_serializes_with_type_tag() {
        let command = command(PositionOperationType::Split, condition_outcomes());
        let json = serde_json::to_value(&command).unwrap();
        assert_eq!(json["type"], "SubmitPositionOperation");
    }
}

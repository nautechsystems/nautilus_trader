//! Domain models for participant discovery and public profile snapshots.

use nautilus_core::{
    UnixNanos,
    correctness::{CorrectnessResult, CorrectnessResultExt, FAILED, check_predicate_true},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};
use ustr::Ustr;

use crate::{
    data::HasTsInit,
    identifiers::{InstrumentId, ParticipantId, Venue},
    reports::{OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price},
};

/// The real-world form represented by a participant identifier.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
#[repr(u8)]
pub enum ParticipantKind {
    /// A blockchain or venue wallet address.
    Wallet,
    /// A natural person.
    Person,
    /// A company, fund, or other organization.
    Organization,
}

/// The current profile enrichment state for a participant.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
#[repr(u8)]
pub enum ProfileState {
    /// No profile has been fetched yet.
    #[default]
    Missing,
    /// A profile fetch has been claimed and is running.
    InFlight,
    /// The latest fetch succeeded.
    Ready,
    /// A transient failure occurred; waiting for another attempt.
    Retry,
    /// Permanent or exhausted failure; no automatic scheduling.
    Failed,
}

/// An identity discovered from public venue activity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct Participant {
    /// The canonical participant identifier.
    pub id: ParticipantId,
    /// The venue where the participant was discovered.
    pub venue: Venue,
    /// The participant kind.
    pub kind: ParticipantKind,
    /// UNIX timestamp (nanoseconds) when the participant was first observed.
    pub first_seen_at: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the participant was most recently observed.
    pub last_seen_at: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
    /// Optional structured metadata (e.g. tags, labels, external enrichment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Participant {
    /// Creates a new [`Participant`] with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `last_seen_at` precedes `first_seen_at`.
    pub fn new_checked(
        id: ParticipantId,
        venue: Venue,
        kind: ParticipantKind,
        first_seen_at: UnixNanos,
        last_seen_at: UnixNanos,
        ts_init: UnixNanos,
    ) -> CorrectnessResult<Self> {
        check_predicate_true(
            first_seen_at <= last_seen_at,
            "`last_seen_at` must not precede `first_seen_at`",
        )?;

        Ok(Self {
            id,
            venue,
            kind,
            first_seen_at,
            last_seen_at,
            ts_init,
            metadata: None,
        })
    }

    /// Creates a new [`Participant`].
    ///
    /// # Panics
    ///
    /// Panics if `last_seen_at` precedes `first_seen_at`.
    #[must_use]
    pub fn new(
        id: ParticipantId,
        venue: Venue,
        kind: ParticipantKind,
        first_seen_at: UnixNanos,
        last_seen_at: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(id, venue, kind, first_seen_at, last_seen_at, ts_init)
            .expect_display(FAILED)
    }

    /// Returns a new participant with the given metadata attached.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl HasTsInit for Participant {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// The method or action type of a participant transaction.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
#[repr(u8)]
pub enum TransactionMethod {
    /// Open a long position.
    OpenLong,
    /// Open a short position.
    OpenShort,
    /// Close a long position.
    CloseLong,
    /// Close a short position.
    CloseShort,
    /// Reverse a long position into a short position.
    LongToShort,
    /// Reverse a short position into a long position.
    ShortToLong,
    /// Auto-deleveraging counterparty execution.
    AutoDeleveraging,
    /// Net child-vault positions into the parent vault.
    NetChildVaults,
    /// Time-weighted average price execution.
    Twap,
    /// Buy an asset.
    Buy,
    /// Sell an asset.
    Sell,
    /// Convert a residual spot balance into its quote asset.
    SpotDustConversion,
    /// Settle an instrument or position.
    Settlement,
    /// Split quote collateral into outcome tokens.
    SplitOutcome,
    /// Merge paired outcome tokens into quote collateral.
    MergeOutcome,
    /// Merge a complete question's outcome tokens into quote collateral.
    MergeQuestion,
    /// Negate one outcome into the complementary outcomes.
    NegateOutcome,
    /// A deposit or transfer in.
    Deposit,
    /// A withdrawal or transfer out.
    Withdraw,
    /// Transfer between participants or accounts.
    Transfer,
    /// A liquidation.
    Liquidation,
    /// Convert one asset or position form into another.
    Conversion,
    /// Another normalized transaction method.
    Other,
}

/// A transaction associated with a participant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct ParticipantTransaction {
    /// The venue or blockchain transaction hash.
    pub hash: Ustr,
    /// The transaction method.
    pub method: TransactionMethod,
    /// UNIX timestamp (nanoseconds) when the transaction occurred.
    pub ts_event: UnixNanos,
    /// The signed token amount from the profile participant's perspective.
    pub amount: Decimal,
    /// The instrument traded.
    pub instrument_id: InstrumentId,
    /// The transaction price.
    pub price: Price,
    /// The monetary value of the transaction.
    pub value: Money,
}

impl ParticipantTransaction {
    /// Creates a new [`ParticipantTransaction`].
    #[must_use]
    pub const fn new(
        hash: Ustr,
        method: TransactionMethod,
        ts_event: UnixNanos,
        amount: Decimal,
        instrument_id: InstrumentId,
        price: Price,
        value: Money,
    ) -> Self {
        Self {
            hash,
            method,
            ts_event,
            amount,
            instrument_id,
            price,
            value,
        }
    }
}

/// A bounded snapshot of public data currently available for a participant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct ParticipantProfile {
    /// The participant associated with this profile.
    pub participant_id: ParticipantId,
    /// Public balances, or `None` when unavailable.
    /// NOTE: could be multiple accounts per participant with multiple currencies
    pub balances: Option<Vec<AccountBalance>>,
    /// Public margin balances, or `None` when unavailable.
    pub margins: Option<Vec<MarginBalance>>,
    /// Open positions, or `None` when unavailable.
    pub positions: Option<Vec<PositionStatusReport>>,
    /// Open orders, or `None` when unavailable.
    pub open_orders: Option<Vec<OrderStatusReport>>,
    /// A bounded window of transactions, or `None` when unavailable.
    pub transactions: Option<Vec<ParticipantTransaction>>,
    /// UNIX timestamp (nanoseconds) when the snapshot was initialized.
    pub ts_init: UnixNanos,
}

impl ParticipantProfile {
    /// Creates a new [`ParticipantProfile`] snapshot.
    #[must_use]
    pub fn new(
        participant_id: ParticipantId,
        balances: Option<Vec<AccountBalance>>,
        margins: Option<Vec<MarginBalance>>,
        positions: Option<Vec<PositionStatusReport>>,
        open_orders: Option<Vec<OrderStatusReport>>,
        transactions: Option<Vec<ParticipantTransaction>>,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            participant_id,
            balances,
            margins,
            positions,
            open_orders,
            transactions,
            ts_init,
        }
    }
}

impl HasTsInit for ParticipantProfile {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::types::Currency;

    const PARTICIPANT_ID: &str = "0x0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn test_participant_rejects_reversed_timestamps() {
        let result = Participant::new_checked(
            ParticipantId::new(PARTICIPANT_ID),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(2),
            UnixNanos::from(1),
            UnixNanos::from(3),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_participant_serialization_roundtrip() {
        let participant = Participant::new(
            ParticipantId::new(PARTICIPANT_ID),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
        );
        let json = serde_json::to_string(&participant).unwrap();
        let decoded: Participant = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, participant);
    }

    #[test]
    fn test_participant_profile_tracks_available_data() {
        let transaction = ParticipantTransaction::new(
            Ustr::from("0x01"),
            TransactionMethod::OpenLong,
            UnixNanos::from(2),
            dec!(0.575),
            InstrumentId::from("KIOXIA-USD-PERP.HYPERLIQUID"),
            Price::new(412.64, 2),
            Money::new(237.27, Currency::USD()),
        );
        let profile = ParticipantProfile::new(
            ParticipantId::new(PARTICIPANT_ID),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            None,
            Some(vec![transaction]),
            UnixNanos::from(3),
        );

        assert_eq!(profile.balances, Some(Vec::new()));
        assert_eq!(profile.margins, Some(Vec::new()));
        assert!(profile.positions.is_none());
        assert_eq!(profile.transactions.as_ref().unwrap().len(), 1);

        let json = serde_json::to_string(&profile).unwrap();
        let decoded: ParticipantProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, profile);
    }
}

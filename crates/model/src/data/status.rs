// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! An `InstrumentStatus` data type representing a change in an instrument market status.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use derive_builder::Builder;
use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::HasTsInit;
use crate::{enums::MarketStatusAction, identifiers::InstrumentId};

/// Represents an event that indicates a change in an instrument market status.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct InstrumentStatus {
    /// The instrument ID for the status change.
    pub instrument_id: InstrumentId,
    /// The instrument market status action.
    pub action: MarketStatusAction,
    /// UNIX timestamp (nanoseconds) when the status event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
    /// Additional details about the cause of the status change.
    pub reason: Option<Ustr>,
    /// Further information about the status change (if provided).
    pub trading_event: Option<Ustr>,
    /// The state of trading in the instrument.
    pub is_trading: Option<bool>,
    /// The state of quoting in the instrument.
    pub is_quoting: Option<bool>,
    /// The state of short sell restrictions for the instrument (if applicable).
    pub is_short_sell_restricted: Option<bool>,
}

impl InstrumentStatus {
    /// Creates a new [`InstrumentStatus`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        action: MarketStatusAction,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reason: Option<Ustr>,
        trading_event: Option<Ustr>,
        is_trading: Option<bool>,
        is_quoting: Option<bool>,
        is_short_sell_restricted: Option<bool>,
    ) -> Self {
        Self {
            instrument_id,
            action,
            ts_event,
            ts_init,
            reason,
            trading_event,
            is_trading,
            is_quoting,
            is_short_sell_restricted,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(instrument_id: &InstrumentId) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata
    }
}

// TODO: Revisit this
impl Display for InstrumentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.instrument_id, self.action, self.ts_event, self.ts_init,
        )
    }
}

impl Serializable for InstrumentStatus {}

impl HasTsInit for InstrumentStatus {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::data::stubs::stub_instrument_status;

    fn create_test_instrument_status() -> InstrumentStatus {
        InstrumentStatus::new(
            InstrumentId::from("EURUSD.SIM"),
            MarketStatusAction::Trading,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            Some(Ustr::from("Normal trading")),
            Some(Ustr::from("MARKET_OPEN")),
            Some(true),
            Some(true),
            Some(false),
        )
    }

    fn create_test_instrument_status_minimal() -> InstrumentStatus {
        InstrumentStatus::new(
            InstrumentId::from("GBPUSD.SIM"),
            MarketStatusAction::PreOpen,
            UnixNanos::from(500_000_000),
            UnixNanos::from(1_000_000_000),
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[rstest]
    fn test_instrument_status_new() {
        let status = create_test_instrument_status();

        assert_eq!(status.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(status.action, MarketStatusAction::Trading);
        assert_eq!(status.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(status.ts_init, UnixNanos::from(2_000_000_000));
        assert_eq!(status.reason, Some(Ustr::from("Normal trading")));
        assert_eq!(status.trading_event, Some(Ustr::from("MARKET_OPEN")));
        assert_eq!(status.is_trading, Some(true));
        assert_eq!(status.is_quoting, Some(true));
        assert_eq!(status.is_short_sell_restricted, Some(false));
    }

    #[rstest]
    fn test_instrument_status_new_minimal() {
        let status = create_test_instrument_status_minimal();

        assert_eq!(status.instrument_id, InstrumentId::from("GBPUSD.SIM"));
        assert_eq!(status.action, MarketStatusAction::PreOpen);
        assert_eq!(status.ts_event, UnixNanos::from(500_000_000));
        assert_eq!(status.ts_init, UnixNanos::from(1_000_000_000));
        assert_eq!(status.reason, None);
        assert_eq!(status.trading_event, None);
        assert_eq!(status.is_trading, None);
        assert_eq!(status.is_quoting, None);
        assert_eq!(status.is_short_sell_restricted, None);
    }

    #[rstest]
    fn test_instrument_status_builder() {
        let status = InstrumentStatusBuilder::default()
            .instrument_id(InstrumentId::from("BTCUSD.CRYPTO"))
            .action(MarketStatusAction::Halt)
            .ts_event(UnixNanos::from(3_000_000_000))
            .ts_init(UnixNanos::from(4_000_000_000))
            .reason(Some(Ustr::from("Technical issue")))
            .trading_event(Some(Ustr::from("HALT_REQUESTED")))
            .is_trading(Some(false))
            .is_quoting(Some(false))
            .is_short_sell_restricted(Some(true))
            .build()
            .unwrap();

        assert_eq!(status.instrument_id, InstrumentId::from("BTCUSD.CRYPTO"));
        assert_eq!(status.action, MarketStatusAction::Halt);
        assert_eq!(status.ts_event, UnixNanos::from(3_000_000_000));
        assert_eq!(status.ts_init, UnixNanos::from(4_000_000_000));
        assert_eq!(status.reason, Some(Ustr::from("Technical issue")));
        assert_eq!(status.trading_event, Some(Ustr::from("HALT_REQUESTED")));
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(status.is_quoting, Some(false));
        assert_eq!(status.is_short_sell_restricted, Some(true));
    }

    #[rstest]
    fn test_instrument_status_builder_minimal() {
        let status = InstrumentStatusBuilder::default()
            .instrument_id(InstrumentId::from("AAPL.XNAS"))
            .action(MarketStatusAction::Close)
            .ts_event(UnixNanos::from(1_500_000_000))
            .ts_init(UnixNanos::from(2_500_000_000))
            .reason(None)
            .trading_event(None)
            .is_trading(None)
            .is_quoting(None)
            .is_short_sell_restricted(None)
            .build()
            .unwrap();

        assert_eq!(status.instrument_id, InstrumentId::from("AAPL.XNAS"));
        assert_eq!(status.action, MarketStatusAction::Close);
        assert_eq!(status.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(status.ts_init, UnixNanos::from(2_500_000_000));
        assert_eq!(status.reason, None);
        assert_eq!(status.trading_event, None);
        assert_eq!(status.is_trading, None);
        assert_eq!(status.is_quoting, None);
        assert_eq!(status.is_short_sell_restricted, None);
    }

    #[rstest]
    #[case(MarketStatusAction::None)]
    #[case(MarketStatusAction::PreOpen)]
    #[case(MarketStatusAction::PreCross)]
    #[case(MarketStatusAction::Quoting)]
    #[case(MarketStatusAction::Cross)]
    #[case(MarketStatusAction::Rotation)]
    #[case(MarketStatusAction::NewPriceIndication)]
    #[case(MarketStatusAction::Trading)]
    #[case(MarketStatusAction::Halt)]
    #[case(MarketStatusAction::Pause)]
    #[case(MarketStatusAction::Suspend)]
    #[case(MarketStatusAction::PreClose)]
    #[case(MarketStatusAction::Close)]
    #[case(MarketStatusAction::PostClose)]
    #[case(MarketStatusAction::ShortSellRestrictionChange)]
    #[case(MarketStatusAction::NotAvailableForTrading)]
    fn test_instrument_status_with_all_actions(#[case] action: MarketStatusAction) {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            action,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(status.action, action);
    }

    #[rstest]
    fn test_get_metadata() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let metadata = InstrumentStatus::get_metadata(&instrument_id);

        assert_eq!(metadata.len(), 1);
        assert_eq!(
            metadata.get("instrument_id"),
            Some(&"EURUSD.SIM".to_string())
        );
    }

    #[rstest]
    fn test_get_metadata_different_instruments() {
        let eur_metadata = InstrumentStatus::get_metadata(&InstrumentId::from("EURUSD.SIM"));
        let gbp_metadata = InstrumentStatus::get_metadata(&InstrumentId::from("GBPUSD.SIM"));

        assert_eq!(
            eur_metadata.get("instrument_id"),
            Some(&"EURUSD.SIM".to_string())
        );
        assert_eq!(
            gbp_metadata.get("instrument_id"),
            Some(&"GBPUSD.SIM".to_string())
        );
        assert_ne!(eur_metadata, gbp_metadata);
    }

    #[rstest]
    fn test_instrument_status_partial_eq() {
        let status1 = create_test_instrument_status();
        let status2 = create_test_instrument_status();
        let status3 = create_test_instrument_status_minimal();

        assert_eq!(status1, status2);
        assert_ne!(status1, status3);
    }

    #[rstest]
    fn test_instrument_status_partial_eq_different_fields() {
        let status1 = create_test_instrument_status();
        let mut status2 = create_test_instrument_status();
        status2.action = MarketStatusAction::Halt;

        let mut status3 = create_test_instrument_status();
        status3.is_trading = Some(false);

        let mut status4 = create_test_instrument_status();
        status4.reason = Some(Ustr::from("Different reason"));

        assert_ne!(status1, status2);
        assert_ne!(status1, status3);
        assert_ne!(status1, status4);
    }

    #[rstest]
    fn test_instrument_status_eq_consistency() {
        let status1 = create_test_instrument_status();
        let status2 = create_test_instrument_status();

        assert_eq!(status1, status2);
        assert_eq!(status2, status1); // Symmetry
        assert_eq!(status1, status1); // Reflexivity
    }

    #[rstest]
    fn test_instrument_status_hash() {
        let status1 = create_test_instrument_status();
        let status2 = create_test_instrument_status();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        status1.hash(&mut hasher1);
        status2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_instrument_status_hash_different_objects() {
        let status1 = create_test_instrument_status();
        let status2 = create_test_instrument_status_minimal();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        status1.hash(&mut hasher1);
        status2.hash(&mut hasher2);

        assert_ne!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_instrument_status_clone() {
        let status1 = create_test_instrument_status();
        let status2 = status1;

        assert_eq!(status1, status2);
        assert_eq!(status1.instrument_id, status2.instrument_id);
        assert_eq!(status1.action, status2.action);
        assert_eq!(status1.ts_event, status2.ts_event);
        assert_eq!(status1.ts_init, status2.ts_init);
        assert_eq!(status1.reason, status2.reason);
        assert_eq!(status1.trading_event, status2.trading_event);
        assert_eq!(status1.is_trading, status2.is_trading);
        assert_eq!(status1.is_quoting, status2.is_quoting);
        assert_eq!(
            status1.is_short_sell_restricted,
            status2.is_short_sell_restricted
        );
    }

    #[rstest]
    fn test_instrument_status_debug() {
        let status = create_test_instrument_status();
        let debug_str = format!("{status:?}");

        assert!(debug_str.contains("InstrumentStatus"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("Trading"));
        assert!(debug_str.contains("Normal trading"));
        assert!(debug_str.contains("MARKET_OPEN"));
    }

    #[rstest]
    fn test_instrument_status_copy() {
        let status1 = create_test_instrument_status();
        let status2 = status1; // Copy, not clone

        assert_eq!(status1, status2);
        assert_eq!(status1.instrument_id, status2.instrument_id);
        assert_eq!(status1.action, status2.action);
    }

    #[rstest]
    fn test_instrument_status_has_ts_init() {
        let status = create_test_instrument_status();
        assert_eq!(status.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_instrument_status_has_ts_init_different_values() {
        let status1 = create_test_instrument_status();
        let status2 = create_test_instrument_status_minimal();

        assert_eq!(status1.ts_init(), UnixNanos::from(2_000_000_000));
        assert_eq!(status2.ts_init(), UnixNanos::from(1_000_000_000));
        assert_ne!(status1.ts_init(), status2.ts_init());
    }

    #[rstest]
    fn test_instrument_status_display() {
        let status = create_test_instrument_status();
        let display_str = format!("{status}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("TRADING"));
        assert!(display_str.contains("1000000000"));
        assert!(display_str.contains("2000000000"));
    }

    #[rstest]
    fn test_instrument_status_display_format() {
        let status = create_test_instrument_status();
        let expected = "EURUSD.SIM,TRADING,1000000000,2000000000";

        assert_eq!(format!("{status}"), expected);
    }

    #[rstest]
    fn test_instrument_status_display_different_actions() {
        let halt_status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Halt,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
            None,
            None,
            None,
            None,
        );

        let display_str = format!("{halt_status}");
        assert!(display_str.contains("HALT"));
    }

    #[rstest]
    fn test_instrument_status_serialization() {
        let status = create_test_instrument_status();

        // Test serde JSON serialization
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: InstrumentStatus = serde_json::from_str(&json).unwrap();

        assert_eq!(status, deserialized);
    }

    #[rstest]
    fn test_instrument_status_serialization_with_optional_fields() {
        let status = create_test_instrument_status_minimal();

        // Test serde JSON serialization with None values
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: InstrumentStatus = serde_json::from_str(&json).unwrap();

        assert_eq!(status, deserialized);
        assert_eq!(deserialized.reason, None);
        assert_eq!(deserialized.trading_event, None);
        assert_eq!(deserialized.is_trading, None);
        assert_eq!(deserialized.is_quoting, None);
        assert_eq!(deserialized.is_short_sell_restricted, None);
    }

    #[rstest]
    fn test_instrument_status_with_trading_flags() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Trading,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            None,
            None,
            Some(true),
            Some(true),
            Some(false),
        );

        assert_eq!(status.is_trading, Some(true));
        assert_eq!(status.is_quoting, Some(true));
        assert_eq!(status.is_short_sell_restricted, Some(false));
    }

    #[rstest]
    fn test_instrument_status_with_halt_flags() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Halt,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            Some(Ustr::from("System maintenance")),
            Some(Ustr::from("HALT_SYSTEM")),
            Some(false),
            Some(false),
            Some(true),
        );

        assert_eq!(status.action, MarketStatusAction::Halt);
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(status.is_quoting, Some(false));
        assert_eq!(status.is_short_sell_restricted, Some(true));
        assert_eq!(status.reason, Some(Ustr::from("System maintenance")));
        assert_eq!(status.trading_event, Some(Ustr::from("HALT_SYSTEM")));
    }

    #[rstest]
    fn test_instrument_status_with_short_sell_restriction() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::ShortSellRestrictionChange,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            Some(Ustr::from("Circuit breaker triggered")),
            Some(Ustr::from("SSR_ACTIVATED")),
            Some(true),
            Some(true),
            Some(true),
        );

        assert_eq!(
            status.action,
            MarketStatusAction::ShortSellRestrictionChange
        );
        assert_eq!(status.is_short_sell_restricted, Some(true));
        assert_eq!(status.reason, Some(Ustr::from("Circuit breaker triggered")));
        assert_eq!(status.trading_event, Some(Ustr::from("SSR_ACTIVATED")));
    }

    #[rstest]
    fn test_instrument_status_with_mixed_optional_fields() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Quoting,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            Some(Ustr::from("Pre-market")),
            None,
            Some(false),
            Some(true),
            None,
        );

        assert_eq!(status.reason, Some(Ustr::from("Pre-market")));
        assert_eq!(status.trading_event, None);
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(status.is_quoting, Some(true));
        assert_eq!(status.is_short_sell_restricted, None);
    }

    #[rstest]
    fn test_instrument_status_with_empty_reason() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Trading,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            Some(Ustr::from("")),
            None,
            None,
            None,
            None,
        );

        assert_eq!(status.reason, Some(Ustr::from("")));
    }

    #[rstest]
    fn test_instrument_status_with_long_reason() {
        let long_reason = "This is a very long reason that explains in detail why the market status has changed and includes multiple sentences to test the handling of longer text strings.";
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Suspend,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            Some(Ustr::from(long_reason)),
            None,
            None,
            None,
            None,
        );

        assert_eq!(status.reason, Some(Ustr::from(long_reason)));
    }

    #[rstest]
    fn test_instrument_status_with_zero_timestamps() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::None,
            UnixNanos::from(0),
            UnixNanos::from(0),
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(status.ts_event, UnixNanos::from(0));
        assert_eq!(status.ts_init, UnixNanos::from(0));
    }

    #[rstest]
    fn test_instrument_status_with_max_timestamps() {
        let status = InstrumentStatus::new(
            InstrumentId::from("TEST.SIM"),
            MarketStatusAction::Trading,
            UnixNanos::from(u64::MAX),
            UnixNanos::from(u64::MAX),
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(status.ts_event, UnixNanos::from(u64::MAX));
        assert_eq!(status.ts_init, UnixNanos::from(u64::MAX));
    }

    #[rstest]
    fn test_to_string(stub_instrument_status: InstrumentStatus) {
        assert_eq!(stub_instrument_status.to_string(), "MSFT.XNAS,TRADING,1,2");
    }
}

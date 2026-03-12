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

//! Betfair-specific custom data types.
//!
//! These types carry Betfair domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData). Each type uses the
//! `#[custom_data(pyo3)]` macro which generates `CustomDataTrait`, Arrow codec, and
//! serialization implementations.
//!
//! Call [`register_betfair_custom_data`] once (e.g. during client `connect()`)
//! to register all types for JSON and Arrow encoding.
//!
//! Absent optional float values use `f64::NAN` as the sentinel, matching
//! Betfair's convention for missing starting price values.

use nautilus_core::UnixNanos;
use nautilus_model::identifiers::InstrumentId;
use nautilus_persistence_macros::custom_data;

/// Serde helpers for f64 fields that use NaN as a sentinel for absent values.
/// Serializes NaN as JSON `null` and deserializes `null` back to NaN,
/// avoiding `serde_json` errors on non-finite floats.
mod nan_as_null {
    pub fn serialize<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if v.is_nan() {
            s.serialize_none()
        } else {
            s.serialize_f64(*v)
        }
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        use serde::Deserialize;
        Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
    }
}

/// Betfair ticker data from MCM runner changes.
///
/// Carries last traded price, traded volume, and starting price
/// near/far values per runner. Fields are `f64::NAN` when absent.
#[custom_data(pyo3)]
pub struct BetfairTicker {
    /// The instrument ID for this ticker.
    pub instrument_id: InstrumentId,
    /// Last traded price.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub last_traded_price: f64,
    /// Total traded volume.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub traded_volume: f64,
    /// Starting price near (projected BSP from matched portion).
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub starting_price_near: f64,
    /// Starting price far (projected BSP from unmatched portion).
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub starting_price_far: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Realized Betfair Starting Price (BSP) for a runner.
///
/// Emitted from the market definition when a runner's BSP is determined.
#[custom_data(pyo3)]
pub struct BetfairStartingPrice {
    /// The instrument ID for this starting price.
    pub instrument_id: InstrumentId,
    /// The realized best starting price value.
    pub bsp: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// BSP order book delta from starting price back/lay arrays.
///
/// Mirrors `OrderBookDelta` fields as a custom data type so strategies
/// can subscribe specifically to BSP book updates (spb/spl) separately
/// from the exchange order book (atb/atl).
#[custom_data(pyo3)]
pub struct BetfairBspBookDelta {
    /// The instrument ID for this BSP delta.
    pub instrument_id: InstrumentId,
    /// The book action (add/update/delete/clear) as `BookAction` u8.
    pub action: u32,
    /// The order side as `OrderSide` u8.
    pub side: u32,
    /// The price level.
    pub price: f64,
    /// The size at this price level.
    pub size: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Marker emitted after all changes in a single MCM batch are processed.
///
/// Strategies can use this to know when a coherent set of market updates
/// has been fully delivered.
#[custom_data(pyo3)]
pub struct BetfairSequenceCompleted {
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Betfair order void event (e.g. VAR void).
///
/// Published when a matched bet is retroactively voided by Betfair, such as
/// when a goal is disallowed following a VAR review.
#[custom_data(pyo3)]
pub struct BetfairOrderVoided {
    /// The instrument ID for the voided order.
    pub instrument_id: InstrumentId,
    /// The client order ID.
    pub client_order_id: String,
    /// The venue (Betfair) order ID (bet ID).
    pub venue_order_id: String,
    /// The size that was voided.
    pub size_voided: f64,
    /// The order price.
    pub price: f64,
    /// The original order size.
    pub size: f64,
    /// The order side ("BACK" or "LAY").
    pub side: String,
    /// The average price matched. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub avg_price_matched: f64,
    /// The total size matched. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub size_matched: f64,
    /// The void reason. Empty string if absent.
    pub reason: String,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// GPS tracking data for a single runner from RCM (Race Change Messages).
///
/// Betfair's Total Performance Data (TPD) provides real-time GPS positions,
/// speed, and stride frequency for each runner in supported races.
#[custom_data(pyo3)]
pub struct BetfairRaceRunnerData {
    /// Race identifier (e.g. "28587288.1650").
    pub race_id: String,
    /// Betfair market identifier.
    pub market_id: String,
    /// Betfair selection identifier.
    pub selection_id: i64,
    /// GPS latitude coordinate. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub latitude: f64,
    /// GPS longitude coordinate. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub longitude: f64,
    /// Speed in m/s (Doppler-derived). `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub speed: f64,
    /// Distance to finish in meters. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub progress: f64,
    /// Stride frequency in Hz. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub stride_frequency: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Race-level progress from RCM (Race Change Messages).
///
/// Provides sectional timing, race order, and obstacle data for the
/// overall race rather than individual runners.
#[custom_data(pyo3)]
pub struct BetfairRaceProgress {
    /// Race identifier (e.g. "28587288.1650").
    pub race_id: String,
    /// Betfair market identifier.
    pub market_id: String,
    /// Gate/sectional name (e.g. "1f", "2f", "Finish"). Empty if absent.
    pub gate_name: String,
    /// Sectional time in seconds. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub sectional_time: f64,
    /// Running time since race start in seconds. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub running_time: f64,
    /// Speed of lead horse in m/s. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub speed: f64,
    /// Distance to finish for leading horse in meters. `f64::NAN` if absent.
    #[serde(
        serialize_with = "nan_as_null::serialize",
        deserialize_with = "nan_as_null::deserialize"
    )]
    pub progress: f64,
    /// Runner order by selection ID (JSON-encoded array). Empty if absent.
    pub order: String,
    /// Jump obstacles (JSON-encoded array of {"J":int,"L":float}). Empty if absent.
    pub jumps: String,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers all Betfair custom data types for JSON and Arrow encoding.
///
/// This must be called once before emitting or persisting Betfair custom data.
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_betfair_custom_data() {
    nautilus_serialization::ensure_custom_data_registered::<BetfairTicker>();
    nautilus_serialization::ensure_custom_data_registered::<BetfairStartingPrice>();
    nautilus_serialization::ensure_custom_data_registered::<BetfairBspBookDelta>();
    nautilus_serialization::ensure_custom_data_registered::<BetfairSequenceCompleted>();
    nautilus_serialization::ensure_custom_data_registered::<BetfairOrderVoided>();
    nautilus_serialization::ensure_custom_data_registered::<BetfairRaceRunnerData>();
    nautilus_serialization::ensure_custom_data_registered::<BetfairRaceProgress>();
}

#[cfg(test)]
mod tests {
    use nautilus_serialization::arrow::ArrowSchemaProvider;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_betfair_ticker_schema() {
        let schema = BetfairTicker::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"instrument_id".to_string()));
        assert!(field_names.contains(&"last_traded_price".to_string()));
        assert!(field_names.contains(&"traded_volume".to_string()));
        assert!(field_names.contains(&"starting_price_near".to_string()));
        assert!(field_names.contains(&"starting_price_far".to_string()));
        assert!(field_names.contains(&"ts_event".to_string()));
        assert!(field_names.contains(&"ts_init".to_string()));
    }

    #[rstest]
    fn test_betfair_starting_price_schema() {
        let schema = BetfairStartingPrice::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"instrument_id".to_string()));
        assert!(field_names.contains(&"bsp".to_string()));
        assert!(field_names.contains(&"ts_event".to_string()));
        assert!(field_names.contains(&"ts_init".to_string()));
    }

    #[rstest]
    fn test_betfair_bsp_book_delta_schema() {
        let schema = BetfairBspBookDelta::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"instrument_id".to_string()));
        assert!(field_names.contains(&"action".to_string()));
        assert!(field_names.contains(&"side".to_string()));
        assert!(field_names.contains(&"price".to_string()));
        assert!(field_names.contains(&"size".to_string()));
        assert!(field_names.contains(&"ts_event".to_string()));
        assert!(field_names.contains(&"ts_init".to_string()));
    }

    #[rstest]
    fn test_betfair_sequence_completed_schema() {
        let schema = BetfairSequenceCompleted::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"ts_event".to_string()));
        assert!(field_names.contains(&"ts_init".to_string()));
    }

    #[rstest]
    fn test_betfair_order_voided_schema() {
        let schema = BetfairOrderVoided::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"instrument_id".to_string()));
        assert!(field_names.contains(&"client_order_id".to_string()));
        assert!(field_names.contains(&"venue_order_id".to_string()));
        assert!(field_names.contains(&"size_voided".to_string()));
        assert!(field_names.contains(&"reason".to_string()));
    }

    #[rstest]
    fn test_register_betfair_custom_data_is_idempotent() {
        register_betfair_custom_data();
        register_betfair_custom_data();
    }

    #[rstest]
    fn test_betfair_race_runner_data_schema() {
        let schema = BetfairRaceRunnerData::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"race_id".to_string()));
        assert!(field_names.contains(&"market_id".to_string()));
        assert!(field_names.contains(&"selection_id".to_string()));
        assert!(field_names.contains(&"latitude".to_string()));
        assert!(field_names.contains(&"longitude".to_string()));
        assert!(field_names.contains(&"speed".to_string()));
        assert!(field_names.contains(&"progress".to_string()));
        assert!(field_names.contains(&"stride_frequency".to_string()));
    }

    #[rstest]
    fn test_betfair_race_progress_schema() {
        let schema = BetfairRaceProgress::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(field_names.contains(&"race_id".to_string()));
        assert!(field_names.contains(&"market_id".to_string()));
        assert!(field_names.contains(&"gate_name".to_string()));
        assert!(field_names.contains(&"sectional_time".to_string()));
        assert!(field_names.contains(&"running_time".to_string()));
        assert!(field_names.contains(&"speed".to_string()));
        assert!(field_names.contains(&"progress".to_string()));
        assert!(field_names.contains(&"order".to_string()));
        assert!(field_names.contains(&"jumps".to_string()));
    }

    #[rstest]
    fn test_race_runner_data_nan_json_roundtrip() {
        let data = BetfairRaceRunnerData::new(
            "28587288.1650".to_string(),
            "1.1234567".to_string(),
            7390417,
            51.4189543,
            -0.4058491,
            17.8,
            f64::NAN,
            f64::NAN,
            UnixNanos::from(1_000_000_000u64),
            UnixNanos::from(1_000_000_000u64),
        );

        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"progress\":null"));
        assert!(json.contains("\"stride_frequency\":null"));
        assert!(json.contains("\"latitude\":51.4189543"));

        let parsed: BetfairRaceRunnerData = serde_json::from_str(&json).unwrap();
        assert!(parsed.progress.is_nan());
        assert!(parsed.stride_frequency.is_nan());
        assert_eq!(parsed.latitude, 51.4189543);
        assert_eq!(parsed.selection_id, 7390417);
    }

    #[rstest]
    fn test_betfair_ticker_nan_json_roundtrip() {
        let ticker = BetfairTicker::new(
            InstrumentId::from("1.234-56789-0.0.BETFAIR"),
            1.5,
            100.0,
            f64::NAN,
            f64::NAN,
            UnixNanos::from(1_000_000_000u64),
            UnixNanos::from(1_000_000_000u64),
        );

        let json = serde_json::to_string(&ticker).unwrap();
        assert!(json.contains("\"starting_price_near\":null"));
        assert!(json.contains("\"starting_price_far\":null"));
        assert!(json.contains("\"last_traded_price\":1.5"));

        let parsed: BetfairTicker = serde_json::from_str(&json).unwrap();
        assert!(parsed.starting_price_near.is_nan());
        assert!(parsed.starting_price_far.is_nan());
        assert_eq!(parsed.last_traded_price, 1.5);
    }
}

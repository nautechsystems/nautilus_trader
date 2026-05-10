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

//! Conversion functions that translate AX API schemas into Nautilus types.

use std::sync::LazyLock;

use ahash::RandomState;
use nautilus_core::nanos::UnixNanos;
pub use nautilus_core::serialization::{
    deserialize_decimal_or_zero, deserialize_optional_decimal_from_str,
    deserialize_optional_decimal_or_zero, deserialize_optional_decimal_str, parse_decimal,
    parse_optional_decimal, serialize_decimal_as_str, serialize_optional_decimal_as_str,
};
use nautilus_model::{
    data::BarSpecification,
    identifiers::ClientOrderId,
    types::{Quantity, fixed::FIXED_PRECISION, quantity::QuantityRaw},
};

use super::enums::AxCandleWidth;

const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;

/// Converts an AX epoch-seconds timestamp to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if `seconds` is negative (malformed data from AX).
pub fn ax_timestamp_s_to_unix_nanos(seconds: i64) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(
        seconds >= 0,
        "AX timestamp must be non-negative, was {seconds}"
    );
    Ok(UnixNanos::from(seconds as u64 * NANOSECONDS_IN_SECOND))
}

/// Converts AX `ts` (seconds) + `tn` (nanoseconds) fields to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if `seconds` is negative (malformed data from AX).
pub fn ax_timestamp_stn_to_unix_nanos(seconds: i64, nanos: i64) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(
        seconds >= 0,
        "AX timestamp must be non-negative, was {seconds}"
    );
    let nanos_part = nanos.max(0) as u64;
    Ok(UnixNanos::from(
        seconds as u64 * NANOSECONDS_IN_SECOND + nanos_part,
    ))
}

/// Converts an AX nanosecond timestamp to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if `nanos` is negative (malformed data from AX).
pub fn ax_timestamp_ns_to_unix_nanos(nanos: i64) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(
        nanos >= 0,
        "AX timestamp_ns must be non-negative, was {nanos}"
    );
    Ok(UnixNanos::from(nanos as u64))
}

/// Cached hasher state for deterministic client order ID to cid conversion
static CID_HASHER: LazyLock<RandomState> = LazyLock::new(|| {
    RandomState::with_seeds(
        0x517cc1b727220a95,
        0x9b5c18c90c3c314d,
        0x5851f42d4c957f2d,
        0x14057b7ef767814f,
    )
});

/// Maps a Nautilus [`BarSpecification`] to an [`AxCandleWidth`].
///
/// # Errors
///
/// Returns an error if the bar specification is not supported by Ax.
pub fn map_bar_spec_to_candle_width(spec: &BarSpecification) -> anyhow::Result<AxCandleWidth> {
    AxCandleWidth::try_from(spec)
}

/// Converts a [`Quantity`] to an i64 contract count for AX orders.
///
/// AX uses integer contracts only. Uses integer arithmetic to avoid
/// floating-point precision issues.
///
/// # Errors
///
/// Returns an error if:
/// - The quantity represents a fractional number of contracts.
/// - The quantity is zero.
pub fn quantity_to_contracts(quantity: Quantity) -> anyhow::Result<u64> {
    let raw = quantity.raw;
    let scale = 10_u64.pow(FIXED_PRECISION as u32) as QuantityRaw;

    // AX requires whole contract quantities
    if !raw.is_multiple_of(scale) {
        anyhow::bail!(
            "AX requires whole contract quantities, was {}",
            quantity.as_f64()
        );
    }

    // QuantityRaw is u128 under the `high-precision` feature and u64 otherwise,
    // so the narrowing cast is conditional on the active feature set.
    #[allow(clippy::unnecessary_cast)]
    let contracts = (raw / scale) as u64;
    if contracts == 0 {
        anyhow::bail!("Order quantity must be at least 1 contract");
    }
    Ok(contracts)
}

/// Converts a [`ClientOrderId`] to a 64-bit unsigned integer for AX `cid` field.
///
/// Uses a deterministic hash of the client order ID string to produce
/// a u64 value that can be used for order correlation.
#[must_use]
pub fn client_order_id_to_cid(client_order_id: &ClientOrderId) -> u64 {
    CID_HASHER.hash_one(client_order_id.inner())
}

/// Creates a [`ClientOrderId`] from a cid value.
///
/// Used when we receive an order with a cid but cannot resolve it to the
/// original ClientOrderId (e.g., after restart when in-memory mapping is lost).
#[must_use]
pub fn cid_to_client_order_id(cid: u64) -> ClientOrderId {
    ClientOrderId::new(format!("CID-{cid}"))
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{BarAggregation, PriceType},
        identifiers::ClientOrderId,
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_client_order_id_to_cid_deterministic() {
        let coid = ClientOrderId::new("O-20240101-000001");

        // Must produce same result across multiple calls
        let cid1 = client_order_id_to_cid(&coid);
        let cid2 = client_order_id_to_cid(&coid);
        let cid3 = client_order_id_to_cid(&coid);

        assert_eq!(cid1, cid2);
        assert_eq!(cid2, cid3);
    }

    #[rstest]
    fn test_client_order_id_to_cid_different_ids() {
        let coid1 = ClientOrderId::new("O-20240101-000001");
        let coid2 = ClientOrderId::new("O-20240101-000002");

        let cid1 = client_order_id_to_cid(&coid1);
        let cid2 = client_order_id_to_cid(&coid2);

        assert_ne!(cid1, cid2);
    }

    #[rstest]
    #[case("O-1")]
    #[case("O-SHORT")]
    #[case("O-20240101-000001")]
    #[case("Order-with-dashes-and-digits-12345")]
    #[case("SINGLE")]
    #[case("a")]
    #[case("X")]
    #[case("LONG-ABCDEFGHIJKLMNOPQRSTUVWXYZ-0123456789")]
    fn test_client_order_id_to_cid_stable_across_varied_inputs(#[case] value: &str) {
        // The hash must be deterministic for any valid ClientOrderId, and the
        // recovered ClientOrderId via cid_to_client_order_id must match the
        // "CID-{cid}" format so reconciliation can resolve lost mappings.
        let coid = ClientOrderId::new(value);
        let cid_a = client_order_id_to_cid(&coid);
        let cid_b = client_order_id_to_cid(&coid);
        assert_eq!(cid_a, cid_b, "hash must be deterministic");

        let recovered = cid_to_client_order_id(cid_a);
        assert!(
            recovered.inner().as_str().starts_with("CID-"),
            "recovered id should have CID prefix: {recovered}",
        );
        assert!(
            !recovered.inner().as_str().is_empty(),
            "recovered id should not be empty",
        );
    }

    #[rstest]
    fn test_client_order_id_to_cid_collision_resistance_small_corpus() {
        // Collision-free over a handful of distinct client order IDs.
        let values = [
            "O-1",
            "O-2",
            "O-10",
            "O-11",
            "O-20240101-000001",
            "O-20240101-000002",
            "strategy-a/1",
            "strategy-a/2",
            "strategy-b/1",
        ];

        let mut seen = std::collections::HashSet::new();
        for v in values {
            let coid = ClientOrderId::new(v);
            let cid = client_order_id_to_cid(&coid);
            assert!(seen.insert(cid), "cid collision for {v}");
        }
    }

    #[rstest]
    fn test_quantity_to_contracts_valid_precision_zero() {
        let qty = Quantity::new(10.0, 0);
        let result = quantity_to_contracts(qty);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10);
    }

    #[rstest]
    fn test_quantity_to_contracts_valid_with_precision() {
        // Whole number with non-zero precision should work
        let qty = Quantity::new(10.0, 2);
        let result = quantity_to_contracts(qty);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10);
    }

    #[rstest]
    fn test_quantity_to_contracts_fractional_rejects() {
        let qty = Quantity::new(10.5, 1);
        let result = quantity_to_contracts(qty);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_quantity_to_contracts_zero_rejects() {
        let qty = Quantity::new(0.0, 0);
        let result = quantity_to_contracts(qty);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_map_bar_spec_1_second() {
        let spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Seconds1));
    }

    #[rstest]
    fn test_map_bar_spec_5_second() {
        let spec = BarSpecification::new(5, BarAggregation::Second, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Seconds5));
    }

    #[rstest]
    fn test_map_bar_spec_1_minute() {
        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Minutes1));
    }

    #[rstest]
    fn test_map_bar_spec_5_minute() {
        let spec = BarSpecification::new(5, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Minutes5));
    }

    #[rstest]
    fn test_map_bar_spec_15_minute() {
        let spec = BarSpecification::new(15, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Minutes15));
    }

    #[rstest]
    fn test_map_bar_spec_1_hour() {
        let spec = BarSpecification::new(1, BarAggregation::Hour, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Hours1));
    }

    #[rstest]
    fn test_map_bar_spec_1_day() {
        let spec = BarSpecification::new(1, BarAggregation::Day, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Days1));
    }

    #[rstest]
    fn test_map_bar_spec_unsupported_step() {
        let spec = BarSpecification::new(3, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_map_bar_spec_unsupported_aggregation() {
        let spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_ax_timestamp_s_to_unix_nanos_valid() {
        let result = ax_timestamp_s_to_unix_nanos(1_000).unwrap();
        assert_eq!(result, UnixNanos::from(1_000_000_000_000u64));
    }

    #[rstest]
    fn test_ax_timestamp_s_to_unix_nanos_zero() {
        let result = ax_timestamp_s_to_unix_nanos(0).unwrap();
        assert_eq!(result, UnixNanos::from(0u64));
    }

    #[rstest]
    fn test_ax_timestamp_s_to_unix_nanos_negative_errors() {
        assert!(ax_timestamp_s_to_unix_nanos(-1).is_err());
    }

    #[rstest]
    fn test_ax_timestamp_ns_to_unix_nanos_valid() {
        let result = ax_timestamp_ns_to_unix_nanos(1_000_000_000).unwrap();
        assert_eq!(result, UnixNanos::from(1_000_000_000u64));
    }

    #[rstest]
    fn test_ax_timestamp_ns_to_unix_nanos_negative_errors() {
        assert!(ax_timestamp_ns_to_unix_nanos(-1).is_err());
    }

    #[rstest]
    fn test_ax_timestamp_stn_to_unix_nanos_combines_seconds_and_nanos() {
        let result = ax_timestamp_stn_to_unix_nanos(1_000, 500).unwrap();
        assert_eq!(result, UnixNanos::from(1_000_000_000_500u64));
    }

    #[rstest]
    fn test_ax_timestamp_stn_to_unix_nanos_zero_nanos() {
        let result = ax_timestamp_stn_to_unix_nanos(1_000, 0).unwrap();
        assert_eq!(result, UnixNanos::from(1_000_000_000_000u64));
    }

    #[rstest]
    fn test_ax_timestamp_stn_to_unix_nanos_negative_seconds_errors() {
        assert!(ax_timestamp_stn_to_unix_nanos(-1, 0).is_err());
    }

    #[rstest]
    fn test_ax_timestamp_stn_to_unix_nanos_negative_nanos_clamps_to_zero() {
        let result = ax_timestamp_stn_to_unix_nanos(1_000, -1).unwrap();
        assert_eq!(result, UnixNanos::from(1_000_000_000_000u64));
    }
}

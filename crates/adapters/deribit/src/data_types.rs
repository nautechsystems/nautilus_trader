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

//! Deribit-specific custom data types.
//!
//! These types carry Deribit domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{InstrumentId, Symbol};
use nautilus_persistence_macros::custom_data;
use rust_decimal::Decimal;

use crate::{common::consts::DERIBIT_VENUE, http::models::DeribitBookSummaryRaw};

/// Deribit volatility index (DVOL) update.
///
/// Emitted from the `deribit_volatility_index.{index_name}` WebSocket channel.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.deribit")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.deribit")
)]
pub struct DeribitVolatilityIndex {
    /// The index identifier (for example `"btc_usd"` or `"eth_usd"`).
    pub index_name: String,
    /// The DVOL value for `index_name`.
    pub volatility: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Book summary snapshot for one instrument from
/// `public/get_book_summary_by_currency`.
///
/// Numeric venue fields are retained as [`Decimal`] (no `f64` round-trip).
/// Convert from the wire DTO via [`DeribitBookSummary::from_raw`].
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.deribit")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.deribit")
)]
pub struct DeribitBookSummary {
    /// Nautilus instrument identifier (venue-qualified).
    pub instrument_id: InstrumentId,
    /// Venue instrument name (e.g. `"BTC-28MAR25-90000-C"`).
    pub instrument_name: String,
    /// Forward/underlying price used for IV calculations.
    #[custom_data_field(serde)]
    pub underlying_price: Option<Decimal>,
    /// Underlying future or index name.
    #[custom_data_field(serde)]
    pub underlying_index: Option<String>,
    /// Mark price.
    #[custom_data_field(serde)]
    pub mark_price: Option<Decimal>,
    /// Mid price.
    #[custom_data_field(serde)]
    pub mid_price: Option<Decimal>,
    /// Best bid price.
    #[custom_data_field(serde)]
    pub bid_price: Option<Decimal>,
    /// Best ask price.
    #[custom_data_field(serde)]
    pub ask_price: Option<Decimal>,
    /// Last traded price.
    #[custom_data_field(serde)]
    pub last_price: Option<Decimal>,
    /// Mark implied volatility.
    #[custom_data_field(serde)]
    pub mark_iv: Option<Decimal>,
    /// Bid implied volatility.
    #[custom_data_field(serde)]
    pub bid_iv: Option<Decimal>,
    /// Ask implied volatility.
    #[custom_data_field(serde)]
    pub ask_iv: Option<Decimal>,
    /// Interest rate used in IV calculations.
    #[custom_data_field(serde)]
    pub interest_rate: Option<Decimal>,
    /// Open interest.
    #[custom_data_field(serde)]
    pub open_interest: Option<Decimal>,
    /// Open interest value when provided.
    #[custom_data_field(serde)]
    pub open_interest_value: Option<Decimal>,
    /// 24h volume.
    #[custom_data_field(serde)]
    pub volume: Option<Decimal>,
    /// 24h volume in USD.
    #[custom_data_field(serde)]
    pub volume_usd: Option<Decimal>,
    /// 24h notional volume.
    #[custom_data_field(serde)]
    pub volume_notional: Option<Decimal>,
    /// 24h volume in BTC when provided.
    #[custom_data_field(serde)]
    pub volume_btc: Option<Decimal>,
    /// 24h high.
    #[custom_data_field(serde)]
    pub high: Option<Decimal>,
    /// 24h low.
    #[custom_data_field(serde)]
    pub low: Option<Decimal>,
    /// 24h price change.
    #[custom_data_field(serde)]
    pub price_change: Option<Decimal>,
    /// Estimated delivery price.
    #[custom_data_field(serde)]
    pub estimated_delivery_price: Option<Decimal>,
    /// Settlement/delivery price when present.
    #[custom_data_field(serde)]
    pub delivery_price: Option<Decimal>,
    /// Base currency.
    #[custom_data_field(serde)]
    pub base_currency: Option<String>,
    /// Quote currency.
    #[custom_data_field(serde)]
    pub quote_currency: Option<String>,
    /// Instrument creation time (milliseconds since UNIX epoch).
    pub creation_timestamp: i64,
    /// UNIX timestamp (nanoseconds) when the snapshot was observed.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl DeribitBookSummary {
    /// Builds a domain book summary from a venue wire DTO.
    #[must_use]
    pub fn from_raw(raw: DeribitBookSummaryRaw, ts: UnixNanos) -> Self {
        let instrument_id = InstrumentId::new(Symbol::new(&raw.instrument_name), *DERIBIT_VENUE);
        Self {
            instrument_id,
            instrument_name: raw.instrument_name,
            underlying_price: raw.underlying_price,
            underlying_index: raw.underlying_index,
            mark_price: raw.mark_price,
            mid_price: raw.mid_price,
            bid_price: raw.bid_price,
            ask_price: raw.ask_price,
            last_price: raw.last_price,
            mark_iv: raw.mark_iv,
            bid_iv: raw.bid_iv,
            ask_iv: raw.ask_iv,
            interest_rate: raw.interest_rate,
            open_interest: raw.open_interest,
            open_interest_value: raw.open_interest_value,
            volume: raw.volume,
            volume_usd: raw.volume_usd,
            volume_notional: raw.volume_notional,
            volume_btc: raw.volume_btc,
            high: raw.high,
            low: raw.low,
            price_change: raw.price_change,
            estimated_delivery_price: raw.estimated_delivery_price,
            delivery_price: raw.delivery_price,
            base_currency: raw.base_currency,
            quote_currency: raw.quote_currency,
            creation_timestamp: raw.creation_timestamp,
            ts_event: ts,
            ts_init: ts,
        }
    }
}

/// Registers Deribit custom data types.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_deribit_custom_data() {
    #[cfg(feature = "arrow")]
    {
        nautilus_serialization::ensure_custom_data_registered::<DeribitVolatilityIndex>();
        nautilus_serialization::ensure_custom_data_registered::<DeribitBookSummary>();
    }

    #[cfg(not(feature = "arrow"))]
    {
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<DeribitVolatilityIndex>();
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<DeribitBookSummary>();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_register_deribit_custom_data_is_idempotent() {
        register_deribit_custom_data();
        register_deribit_custom_data();
    }

    #[rstest]
    fn test_book_summary_from_raw_preserves_decimals_and_instrument_id() {
        let raw = DeribitBookSummaryRaw {
            instrument_name: "BTC-28MAR25-90000-C".to_string(),
            underlying_price: Some(dec!(95000.5)),
            underlying_index: Some("SYN.BTC-28MAR25".to_string()),
            mark_price: Some(dec!(0.042)),
            mid_price: None,
            bid_price: Some(dec!(0.040)),
            ask_price: Some(dec!(0.042)),
            last_price: None,
            mark_iv: Some(dec!(55.2)),
            bid_iv: None,
            ask_iv: None,
            interest_rate: None,
            open_interest: Some(dec!(123.5)),
            open_interest_value: None,
            volume: None,
            volume_usd: None,
            volume_notional: None,
            volume_btc: None,
            high: None,
            low: None,
            price_change: None,
            estimated_delivery_price: None,
            delivery_price: None,
            base_currency: Some("BTC".to_string()),
            quote_currency: Some("USD".to_string()),
            creation_timestamp: 1_710_000_000_000,
        };
        let ts = UnixNanos::from(42_u64);
        let summary = DeribitBookSummary::from_raw(raw, ts);

        assert_eq!(
            summary.instrument_id,
            InstrumentId::from("BTC-28MAR25-90000-C.DERIBIT")
        );
        assert_eq!(summary.mark_iv, Some(dec!(55.2)));
        assert_eq!(summary.open_interest, Some(dec!(123.5)));
        assert_eq!(summary.ts_event, ts);
        assert_eq!(summary.ts_init, ts);
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_deribit_volatility_index_arrow_schema() {
        use arrow::datatypes::DataType;
        use nautilus_serialization::arrow::ArrowSchemaProvider;

        let schema = DeribitVolatilityIndex::get_schema(None);

        assert_eq!(schema.fields().len(), 4);
        assert_eq!(schema.field(0).name(), "index_name");
        assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(1).name(), "volatility");
        assert_eq!(schema.field(1).data_type(), &DataType::Float64);
        assert_eq!(schema.field(2).name(), "ts_event");
        assert_eq!(schema.field(2).data_type(), &DataType::UInt64);
        assert_eq!(schema.field(3).name(), "ts_init");
        assert_eq!(schema.field(3).data_type(), &DataType::UInt64);
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_book_summary_arrow_roundtrip_preserves_decimals() {
        use arrow::datatypes::DataType;
        use nautilus_serialization::arrow::{
            ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch,
        };

        let original = DeribitBookSummary::from_raw(
            DeribitBookSummaryRaw {
                instrument_name: "BTC-28MAR25-90000-C".to_string(),
                underlying_price: Some(dec!(95000.5)),
                underlying_index: Some("SYN.BTC-28MAR25".to_string()),
                mark_price: Some(dec!(0.042)),
                mid_price: None,
                bid_price: Some(dec!(0.040)),
                ask_price: Some(dec!(0.042)),
                last_price: None,
                mark_iv: Some(dec!(55.2)),
                bid_iv: None,
                ask_iv: None,
                interest_rate: None,
                open_interest: Some(dec!(123.5)),
                open_interest_value: None,
                volume: None,
                volume_usd: None,
                volume_notional: None,
                volume_btc: None,
                high: None,
                low: None,
                price_change: None,
                estimated_delivery_price: None,
                delivery_price: None,
                base_currency: Some("BTC".to_string()),
                quote_currency: Some("USD".to_string()),
                creation_timestamp: 1_710_000_000_000,
            },
            UnixNanos::from(1_000_u64),
        );

        // Built-in #[custom_data_field(serde)] path: Option<Decimal> → Utf8.
        let schema = DeribitBookSummary::get_schema(None);
        assert_eq!(
            schema.field_with_name("mark_iv").unwrap().data_type(),
            &DataType::Utf8
        );

        let metadata = original.metadata();
        let batch =
            DeribitBookSummary::encode_batch(&metadata, std::slice::from_ref(&original)).unwrap();
        let decoded = DeribitBookSummary::decode_data_batch(&metadata, batch).unwrap();
        let decoded = DeribitBookSummary::try_from(decoded.into_iter().next().unwrap()).unwrap();
        assert_eq!(decoded, original);
    }
}

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

//! Shared Arrow conversion for compatibility [`Data`] rows.

#[cfg(feature = "python")]
use std::collections::BTreeMap;
use std::sync::Arc;

use arrow::{datatypes::Schema, record_batch::RecordBatch};
#[cfg(feature = "python")]
use nautilus_model::data::{
    Bar, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
    MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick,
    to_variant,
};
use nautilus_model::{
    data::{NautilusDataType, NautilusRecordType},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSnapshot, OrderSubmitted, OrderTriggered, OrderUpdated, PositionAdjusted,
        PositionChanged, PositionClosed, PositionOpened, PositionSnapshot,
    },
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch,
    catalog_display::catalog_display_schema, is_nautilus_legacy_schema,
    schema_with_identifier_column, timestamp_data_type,
};

#[cfg(feature = "python")]
use super::custom::{group_custom_data_by_type, prepare_custom_data_batch};

pub(crate) fn validate_catalog_schema(schema: &Schema) -> anyhow::Result<()> {
    let legacy_timestamps = ["ts_event", "ts_init"].iter().any(|name| {
        schema
            .field_with_name(name)
            .is_ok_and(|field| field.data_type() != &timestamp_data_type())
    });

    anyhow::ensure!(
        !legacy_timestamps && !is_nautilus_legacy_schema(schema),
        "Legacy catalog schema is not supported by runtime queries; run `nautilus catalog migrate-parquet` to migrate to a separate destination before reading"
    );
    Ok(())
}

#[cfg(feature = "python")]
pub(crate) fn data_to_arrow_batches(
    data_type: &NautilusDataType,
    data: Vec<Data>,
) -> anyhow::Result<Vec<RecordBatch>> {
    macro_rules! encode {
        ($type:ty) => {{
            let values = to_variant::<$type>(data);
            encode_grouped_batches(&values)?
        }};
    }

    macro_rules! encode_data_type {
        (
            (Instrument, InstrumentAny, Instrument, Instrument, "instruments"),
            $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?
        ) => {
            match data_type {
                NautilusDataType::Instrument => {
                    anyhow::bail!("Instrument definitions do not have one shared Arrow schema")
                }
                $(NautilusDataType::$variant => encode!($type),)+
                NautilusDataType::Custom { .. } => {
                    let custom = data
                        .iter()
                        .filter_map(|item| match item {
                            Data::Custom(custom) => Some(custom),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let mut batches = Vec::new();
                    for group in group_custom_data_by_type(custom) {
                        batches.push(prepare_custom_data_batch(&group)?.0);
                    }
                    batches
                }
                #[cfg(feature = "defi")]
                NautilusDataType::Defi => {
                    anyhow::bail!("Catalog Arrow queries do not support DeFi data")
                }
                #[cfg(not(feature = "defi"))]
                #[allow(unreachable_patterns, reason = "DeFi variants can exist without this crate's defi feature")]
                _ => anyhow::bail!("Catalog Arrow queries do not support DeFi data"),
            }
        };
    }

    Ok(nautilus_model::for_each_data_type!(encode_data_type))
}

#[cfg(feature = "python")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BatchIdentity {
    identifier: String,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
}

#[cfg(feature = "python")]
impl BatchIdentity {
    fn new(
        identifier: &impl ToString,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
    ) -> Self {
        Self {
            identifier: identifier.to_string(),
            price_precision,
            size_precision,
        }
    }
}

#[cfg(feature = "python")]
trait CatalogBatchIdentity {
    fn batch_identity(&self) -> BatchIdentity;
}

#[cfg(feature = "python")]
macro_rules! impl_batch_identity {
    ($type:ty, $identifier:expr, $price_precision:expr, $size_precision:expr) => {
        impl CatalogBatchIdentity for $type {
            fn batch_identity(&self) -> BatchIdentity {
                BatchIdentity::new(
                    &$identifier(self),
                    $price_precision(self),
                    $size_precision(self),
                )
            }
        }
    };
}

#[cfg(feature = "python")]
impl_batch_identity!(
    QuoteTick,
    |value: &QuoteTick| value.instrument_id,
    |value: &QuoteTick| Some(value.bid_price.precision),
    |value: &QuoteTick| Some(value.bid_size.precision)
);
#[cfg(feature = "python")]
impl_batch_identity!(
    TradeTick,
    |value: &TradeTick| value.instrument_id,
    |value: &TradeTick| Some(value.price.precision),
    |value: &TradeTick| Some(value.size.precision)
);
#[cfg(feature = "python")]
impl_batch_identity!(
    OrderBookDelta,
    |value: &OrderBookDelta| value.instrument_id,
    |value: &OrderBookDelta| Some(value.order.price.precision),
    |value: &OrderBookDelta| Some(value.order.size.precision)
);
#[cfg(feature = "python")]
impl_batch_identity!(
    OrderBookDepth,
    |value: &OrderBookDepth| value.instrument_id,
    |value: &OrderBookDepth| value
        .bids
        .first()
        .or_else(|| value.asks.first())
        .map(|order| order.price.precision),
    |value: &OrderBookDepth| value
        .bids
        .first()
        .or_else(|| value.asks.first())
        .map(|order| order.size.precision)
);
#[cfg(feature = "python")]
impl_batch_identity!(
    Bar,
    |value: &Bar| value.bar_type,
    |value: &Bar| Some(value.open.precision),
    |value: &Bar| Some(value.volume.precision)
);
#[cfg(feature = "python")]
impl_batch_identity!(
    MarkPriceUpdate,
    |value: &MarkPriceUpdate| value.instrument_id,
    |value: &MarkPriceUpdate| Some(value.value.precision),
    |_value: &MarkPriceUpdate| None
);
#[cfg(feature = "python")]
impl_batch_identity!(
    IndexPriceUpdate,
    |value: &IndexPriceUpdate| value.instrument_id,
    |value: &IndexPriceUpdate| Some(value.value.precision),
    |_value: &IndexPriceUpdate| None
);
#[cfg(feature = "python")]
impl_batch_identity!(
    InstrumentClose,
    |value: &InstrumentClose| value.instrument_id,
    |value: &InstrumentClose| Some(value.close_price.precision),
    |_value: &InstrumentClose| None
);
#[cfg(feature = "python")]
impl_batch_identity!(
    FundingRateUpdate,
    |value: &FundingRateUpdate| value.instrument_id,
    |_value: &FundingRateUpdate| None,
    |_value: &FundingRateUpdate| None
);
#[cfg(feature = "python")]
impl_batch_identity!(
    InstrumentStatus,
    |value: &InstrumentStatus| value.instrument_id,
    |_value: &InstrumentStatus| None,
    |_value: &InstrumentStatus| None
);
#[cfg(feature = "python")]
impl_batch_identity!(
    OptionGreeks,
    |value: &OptionGreeks| value.instrument_id,
    |_value: &OptionGreeks| None,
    |_value: &OptionGreeks| None
);

// Groups order lexically and retain input order within each group. Precision is part of the key,
// so one identifier can produce separate batches after a precision change.
#[cfg(feature = "python")]
fn encode_grouped_batches<T>(values: &[T]) -> anyhow::Result<Vec<RecordBatch>>
where
    T: CatalogBatchIdentity + EncodeToRecordBatch,
{
    let mut groups: BTreeMap<BatchIdentity, Vec<&T>> = BTreeMap::new();

    for value in values {
        groups
            .entry(value.batch_identity())
            .or_default()
            .push(value);
    }

    groups
        .into_values()
        .map(|values| {
            let metadata = values[0].metadata();
            T::encode_batch(&metadata, &values).map_err(Into::into)
        })
        .collect()
}

// Expands to `$call::<T>(args)` for the Rust type of each fixed-schema record selector
macro_rules! dispatch_record_type {
    ($record_type:expr, $call:ident($($arg:expr),*)) => {
        match $record_type {
            NautilusRecordType::AccountState => $call::<AccountState>($($arg),*),
            NautilusRecordType::OrderInitialized => $call::<OrderInitialized>($($arg),*),
            NautilusRecordType::OrderDenied => $call::<OrderDenied>($($arg),*),
            NautilusRecordType::OrderEmulated => $call::<OrderEmulated>($($arg),*),
            NautilusRecordType::OrderSubmitted => $call::<OrderSubmitted>($($arg),*),
            NautilusRecordType::OrderAccepted => $call::<OrderAccepted>($($arg),*),
            NautilusRecordType::OrderRejected => $call::<OrderRejected>($($arg),*),
            NautilusRecordType::OrderPendingCancel => $call::<OrderPendingCancel>($($arg),*),
            NautilusRecordType::OrderCanceled => $call::<OrderCanceled>($($arg),*),
            NautilusRecordType::OrderCancelRejected => $call::<OrderCancelRejected>($($arg),*),
            NautilusRecordType::OrderExpired => $call::<OrderExpired>($($arg),*),
            NautilusRecordType::OrderTriggered => $call::<OrderTriggered>($($arg),*),
            NautilusRecordType::OrderPendingUpdate => $call::<OrderPendingUpdate>($($arg),*),
            NautilusRecordType::OrderReleased => $call::<OrderReleased>($($arg),*),
            NautilusRecordType::OrderModifyRejected => $call::<OrderModifyRejected>($($arg),*),
            NautilusRecordType::OrderUpdated => $call::<OrderUpdated>($($arg),*),
            NautilusRecordType::OrderFilled => $call::<OrderFilled>($($arg),*),
            NautilusRecordType::OrderFillVoided => $call::<OrderFillVoided>($($arg),*),
            NautilusRecordType::PositionOpened => $call::<PositionOpened>($($arg),*),
            NautilusRecordType::PositionChanged => $call::<PositionChanged>($($arg),*),
            NautilusRecordType::PositionClosed => $call::<PositionClosed>($($arg),*),
            NautilusRecordType::PositionAdjusted => $call::<PositionAdjusted>($($arg),*),
            NautilusRecordType::OrderSnapshot => $call::<OrderSnapshot>($($arg),*),
            NautilusRecordType::PositionSnapshot => $call::<PositionSnapshot>($($arg),*),
            NautilusRecordType::OrderStatusReport => $call::<OrderStatusReport>($($arg),*),
            NautilusRecordType::FillReport => $call::<FillReport>($($arg),*),
            NautilusRecordType::PositionStatusReport => $call::<PositionStatusReport>($($arg),*),
            NautilusRecordType::ExecutionMassStatus => $call::<ExecutionMassStatus>($($arg),*),
            #[cfg(feature = "defi")]
            NautilusRecordType::Defi => {
                anyhow::bail!("Catalog Arrow queries do not support DeFi records")
            }
            #[cfg(not(feature = "defi"))]
            #[allow(
                unreachable_patterns,
                reason = "DeFi variants can exist without this crate's defi feature"
            )]
            _ => anyhow::bail!("Catalog Arrow queries do not support DeFi records"),
        }
    };
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "DeFi builds reject the non-fixed record selector"
)]
pub(crate) fn catalog_record_schema(record_type: NautilusRecordType) -> anyhow::Result<Schema> {
    Ok(dispatch_record_type!(record_type, record_schema()))
}

fn record_schema<T: ArrowSchemaProvider>() -> Schema {
    T::get_schema(None)
}

pub(crate) fn round_trip_catalog_record_batches(
    record_type: NautilusRecordType,
    batches: Vec<RecordBatch>,
) -> anyhow::Result<Vec<RecordBatch>> {
    dispatch_record_type!(record_type, round_trip_batches(batches))
}

// Round-trips the batches of one output file through `T` with the metadata that the current
// catalog writer selects across all of their values, keeping each source batch's size
pub(crate) fn round_trip_batches<T>(batches: Vec<RecordBatch>) -> anyhow::Result<Vec<RecordBatch>>
where
    T: DecodeTypedFromRecordBatch + EncodeToRecordBatch,
{
    let decoded = batches
        .into_iter()
        .map(|batch| {
            let metadata = batch.schema().metadata().clone();
            T::decode_typed_batch(&metadata, batch)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let values = decoded.iter().flatten().collect::<Vec<_>>();

    anyhow::ensure!(!values.is_empty(), "Cannot re-encode empty record batches");
    let chunk_metadata = T::chunk_metadata(&values);

    if let Some(position) = values
        .iter()
        .position(|value| !value.matches_chunk_metadata(&chunk_metadata))
    {
        anyhow::bail!(
            "Cannot re-encode mixed identities: row {position} has metadata {:?} but the \
             batch has {chunk_metadata:?}",
            values[position].metadata(),
        );
    }

    decoded
        .iter()
        .map(|values| Ok(T::encode_batch(&chunk_metadata, values)?))
        .collect()
}

pub(crate) fn empty_display_batch_with_identifier(
    data_type: &NautilusDataType,
) -> anyhow::Result<RecordBatch> {
    let schema = schema_with_identifier_column(&catalog_display_schema(data_type)?);
    Ok(RecordBatch::new_empty(Arc::new(schema)))
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::{KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION};
    use rstest::rstest;

    use super::*;

    fn quote(instrument_id: &str, price: &str, ts: u64) -> Data {
        Data::Quote(QuoteTick::new(
            InstrumentId::from(instrument_id),
            Price::from(price),
            Price::from(price),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        ))
    }

    #[rstest]
    fn data_to_arrow_batches_groups_by_identifier_and_precision() {
        let data = vec![
            quote("ETHUSDT.BINANCE", "1.00", 1),
            quote("AUD/USD.SIM", "1.00000", 2),
            quote("AUD/USD.SIM", "1.000000", 3),
            quote("AUD/USD.SIM", "1.00001", 4),
        ];

        let batches = data_to_arrow_batches(&NautilusDataType::QuoteTick, data).unwrap();

        let groups = batches
            .iter()
            .map(|batch| {
                let metadata = batch.schema().metadata().clone();
                (
                    metadata[KEY_INSTRUMENT_ID].clone(),
                    metadata[KEY_PRICE_PRECISION].clone(),
                    batch.num_rows(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            groups,
            vec![
                ("AUD/USD.SIM".to_string(), "5".to_string(), 2),
                ("AUD/USD.SIM".to_string(), "6".to_string(), 1),
                ("ETHUSDT.BINANCE".to_string(), "2".to_string(), 1),
            ],
        );
    }

    #[rstest]
    fn data_to_arrow_batches_rejects_instruments() {
        let error = data_to_arrow_batches(&NautilusDataType::Instrument, Vec::new()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Instrument definitions do not have one shared Arrow schema"
        );
    }
}

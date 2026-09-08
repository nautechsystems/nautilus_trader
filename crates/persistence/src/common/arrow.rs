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
#[cfg(feature = "python")]
use nautilus_serialization::arrow::EncodeToRecordBatch;
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, catalog_display::catalog_display_schema, schema_with_identifier_column,
};

#[cfg(feature = "python")]
use super::custom::{group_custom_data_by_type, prepare_custom_data_batch};

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
                NautilusDataType::OrderBook => {
                    anyhow::bail!("Order book snapshots do not have one shared Arrow schema")
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
            }
        };
    }

    Ok(nautilus_model::for_each_data_type!(encode_data_type))
}

#[cfg(feature = "python")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BatchIdentity {
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
pub(crate) trait CatalogBatchIdentity {
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
    |value: &OrderBookDepth| Some(value.bids[0].price.precision),
    |value: &OrderBookDepth| Some(value.bids[0].size.precision)
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
pub(crate) fn encode_grouped_batches<T>(values: &[T]) -> anyhow::Result<Vec<RecordBatch>>
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

#[allow(
    clippy::unnecessary_wraps,
    reason = "DeFi builds reject the non-fixed record selector"
)]
pub(crate) fn catalog_record_schema(record_type: &NautilusRecordType) -> anyhow::Result<Schema> {
    macro_rules! schema {
        ($type:ty) => {
            <$type>::get_schema(None)
        };
    }

    Ok(match record_type {
        NautilusRecordType::AccountState => schema!(AccountState),
        NautilusRecordType::OrderInitialized => schema!(OrderInitialized),
        NautilusRecordType::OrderDenied => schema!(OrderDenied),
        NautilusRecordType::OrderEmulated => schema!(OrderEmulated),
        NautilusRecordType::OrderSubmitted => schema!(OrderSubmitted),
        NautilusRecordType::OrderAccepted => schema!(OrderAccepted),
        NautilusRecordType::OrderRejected => schema!(OrderRejected),
        NautilusRecordType::OrderPendingCancel => schema!(OrderPendingCancel),
        NautilusRecordType::OrderCanceled => schema!(OrderCanceled),
        NautilusRecordType::OrderCancelRejected => schema!(OrderCancelRejected),
        NautilusRecordType::OrderExpired => schema!(OrderExpired),
        NautilusRecordType::OrderTriggered => schema!(OrderTriggered),
        NautilusRecordType::OrderPendingUpdate => schema!(OrderPendingUpdate),
        NautilusRecordType::OrderReleased => schema!(OrderReleased),
        NautilusRecordType::OrderModifyRejected => schema!(OrderModifyRejected),
        NautilusRecordType::OrderUpdated => schema!(OrderUpdated),
        NautilusRecordType::OrderFilled => schema!(OrderFilled),
        NautilusRecordType::OrderFillVoided => schema!(OrderFillVoided),
        NautilusRecordType::PositionOpened => schema!(PositionOpened),
        NautilusRecordType::PositionChanged => schema!(PositionChanged),
        NautilusRecordType::PositionClosed => schema!(PositionClosed),
        NautilusRecordType::PositionAdjusted => schema!(PositionAdjusted),
        NautilusRecordType::OrderSnapshot => schema!(OrderSnapshot),
        NautilusRecordType::PositionSnapshot => schema!(PositionSnapshot),
        NautilusRecordType::OrderStatusReport => schema!(OrderStatusReport),
        NautilusRecordType::FillReport => schema!(FillReport),
        NautilusRecordType::PositionStatusReport => schema!(PositionStatusReport),
        NautilusRecordType::ExecutionMassStatus => schema!(ExecutionMassStatus),
        #[cfg(feature = "defi")]
        NautilusRecordType::Defi => {
            anyhow::bail!("Catalog Arrow queries do not support DeFi records")
        }
    })
}

pub(crate) fn empty_display_batch_with_identifier(
    data_type: &NautilusDataType,
) -> anyhow::Result<RecordBatch> {
    let schema = schema_with_identifier_column(&catalog_display_schema(data_type)?);
    Ok(RecordBatch::new_empty(Arc::new(schema)))
}

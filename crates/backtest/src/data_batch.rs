// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Typed and compatibility batch storage for historical replay.

use nautilus_model::data::{BatchView, Data, DataBatch, DataRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataKind {
    BookDelta,
    BookDeltas,
    BookDepth10,
    Quote,
    Trade,
    Bar,
    MarkPrice,
    IndexPrice,
    FundingRate,
    OptionGreeks,
    InstrumentStatus,
    InstrumentClose,
    Custom,
    #[cfg(feature = "defi")]
    Defi,
}

impl From<&Data> for DataKind {
    fn from(data: &Data) -> Self {
        match data {
            Data::Delta(_) => Self::BookDelta,
            Data::Deltas(_) => Self::BookDeltas,
            Data::Depth10(_) => Self::BookDepth10,
            Data::Quote(_) => Self::Quote,
            Data::Trade(_) => Self::Trade,
            Data::Bar(_) => Self::Bar,
            Data::MarkPrice(_) => Self::MarkPrice,
            Data::IndexPrice(_) => Self::IndexPrice,
            Data::FundingRate(_) => Self::FundingRate,
            Data::OptionGreeks(_) => Self::OptionGreeks,
            Data::InstrumentStatus(_) => Self::InstrumentStatus,
            Data::InstrumentClose(_) => Self::InstrumentClose,
            Data::Custom(_) => Self::Custom,
            #[cfg(feature = "defi")]
            Data::Defi(_) => Self::Defi,
        }
    }
}

macro_rules! collect_batch {
    ($data:expr, $data_variant:ident, $batch_variant:ident) => {
        ReplayBatch::Typed(DataBatch::$batch_variant(BatchView::from(
            $data
                .into_iter()
                .map(|item| match item {
                    Data::$data_variant(value) => value,
                    _ => unreachable!("data kind changed during batch conversion"),
                })
                .collect::<Vec<_>>(),
        )))
    };
    ($data:expr, $data_variant:ident, $batch_variant:ident, boxed) => {
        ReplayBatch::Typed(DataBatch::$batch_variant(BatchView::from(
            $data
                .into_iter()
                .map(|item| match item {
                    Data::$data_variant(value) => *value,
                    _ => unreachable!("data kind changed during batch conversion"),
                })
                .collect::<Vec<_>>(),
        )))
    };
}

/// Replay storage for one logical stream, using typed batches when every item shares a `Data`
/// variant and retaining mixed and custom streams as original `Data` values.
#[derive(Debug)]
pub(super) enum ReplayBatch {
    Compatibility(BatchView<Data>),
    Typed(DataBatch),
}

impl ReplayBatch {
    pub(super) fn from_data(data: Vec<Data>) -> Self {
        let Some(first) = data.first() else {
            return Self::Compatibility(BatchView::from(data));
        };
        let kind = DataKind::from(first);
        if data.iter().any(|item| DataKind::from(item) != kind) {
            return Self::Compatibility(BatchView::from(data));
        }

        match kind {
            DataKind::BookDelta => collect_batch!(data, Delta, BookDelta),
            DataKind::BookDeltas => collect_batch!(data, Deltas, BookDeltas, boxed),
            DataKind::BookDepth10 => collect_batch!(data, Depth10, BookDepth10, boxed),
            DataKind::Quote => collect_batch!(data, Quote, Quote),
            DataKind::Trade => collect_batch!(data, Trade, Trade),
            DataKind::Bar => collect_batch!(data, Bar, Bar),
            DataKind::MarkPrice => collect_batch!(data, MarkPrice, MarkPrice),
            DataKind::IndexPrice => collect_batch!(data, IndexPrice, IndexPrice),
            DataKind::FundingRate => collect_batch!(data, FundingRate, FundingRate),
            DataKind::OptionGreeks => collect_batch!(data, OptionGreeks, OptionGreeks),
            DataKind::InstrumentStatus => {
                collect_batch!(data, InstrumentStatus, InstrumentStatus)
            }
            DataKind::InstrumentClose => {
                collect_batch!(data, InstrumentClose, InstrumentClose)
            }
            DataKind::Custom => Self::Compatibility(BatchView::from(data)),
            #[cfg(feature = "defi")]
            DataKind::Defi => collect_batch!(data, Defi, Defi, boxed),
        }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Compatibility(data) => data.len(),
            Self::Typed(data) => data.len(),
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<DataRef<'_>> {
        match self {
            Self::Compatibility(data) => data.get(index).map(DataRef::from),
            Self::Typed(data) => data.get(index),
        }
    }

    pub(super) fn get_owned(&self, index: usize) -> Option<Data> {
        match self.get(index)? {
            DataRef::BookDelta(data) => Some(Data::Delta(*data)),
            DataRef::BookDeltas(data) => Some(Data::Deltas(Box::new(data.clone()))),
            DataRef::BookDepth10(data) => Some(Data::Depth10(Box::new(*data))),
            DataRef::Quote(data) => Some(Data::Quote(*data)),
            DataRef::Trade(data) => Some(Data::Trade(*data)),
            DataRef::Bar(data) => Some(Data::Bar(*data)),
            DataRef::MarkPrice(data) => Some(Data::MarkPrice(*data)),
            DataRef::IndexPrice(data) => Some(Data::IndexPrice(*data)),
            DataRef::FundingRate(data) => Some(Data::FundingRate(*data)),
            DataRef::OptionGreeks(data) => Some(Data::OptionGreeks(*data)),
            DataRef::InstrumentStatus(data) => Some(Data::InstrumentStatus(*data)),
            DataRef::InstrumentClose(data) => Some(Data::InstrumentClose(*data)),
            DataRef::Custom(data) => Some(Data::Custom(data.clone())),
            #[cfg(feature = "defi")]
            DataRef::Defi(data) => Some(Data::Defi(Box::new(data.clone()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{
            Data, DataRef, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OptionGreeks,
            QuoteTick,
            stubs::{
                stub_bar, stub_custom_data, stub_delta, stub_deltas, stub_depth10,
                stub_instrument_close, stub_instrument_status, stub_trade_ethusdt_buy,
            },
        },
        identifiers::InstrumentId,
        types::Price,
    };
    #[cfg(feature = "defi")]
    use nautilus_model::{
        defi::{
            DefiData,
            data::block::BlockPosition,
            pool_analysis::snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
        },
        identifiers::{Symbol, Venue},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_homogeneous_data_uses_typed_storage_for_every_non_defi_static_variant() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let data = vec![
            Data::Delta(stub_delta()),
            Data::Deltas(Box::new(stub_deltas())),
            Data::Depth10(Box::new(stub_depth10())),
            Data::Quote(QuoteTick::default()),
            Data::Trade(stub_trade_ethusdt_buy()),
            Data::Bar(stub_bar()),
            Data::MarkPrice(MarkPriceUpdate::new(
                instrument_id,
                Price::from("100.10"),
                UnixNanos::from(7),
                UnixNanos::from(8),
            )),
            Data::IndexPrice(IndexPriceUpdate::new(
                instrument_id,
                Price::from("100.20"),
                UnixNanos::from(9),
                UnixNanos::from(10),
            )),
            Data::FundingRate(FundingRateUpdate::new(
                instrument_id,
                "0.0001".parse().unwrap(),
                Some(480),
                Some(UnixNanos::from(12)),
                UnixNanos::from(11),
                UnixNanos::from(12),
            )),
            Data::OptionGreeks(OptionGreeks {
                instrument_id,
                ts_event: UnixNanos::from(13),
                ts_init: UnixNanos::from(14),
                ..OptionGreeks::default()
            }),
            Data::InstrumentStatus(stub_instrument_status()),
            Data::InstrumentClose(stub_instrument_close()),
        ];
        assert_eq!(data.len(), 12, "every static Data variant needs a case");

        for item in data {
            let batch = ReplayBatch::from_data(vec![item]);

            assert_eq!(batch.len(), 1);

            match (&batch, batch.get(0)) {
                (ReplayBatch::Typed(DataBatch::BookDelta(_)), Some(DataRef::BookDelta(_)))
                | (ReplayBatch::Typed(DataBatch::BookDeltas(_)), Some(DataRef::BookDeltas(_)))
                | (ReplayBatch::Typed(DataBatch::BookDepth10(_)), Some(DataRef::BookDepth10(_)))
                | (ReplayBatch::Typed(DataBatch::Quote(_)), Some(DataRef::Quote(_)))
                | (ReplayBatch::Typed(DataBatch::Trade(_)), Some(DataRef::Trade(_)))
                | (ReplayBatch::Typed(DataBatch::Bar(_)), Some(DataRef::Bar(_)))
                | (ReplayBatch::Typed(DataBatch::MarkPrice(_)), Some(DataRef::MarkPrice(_)))
                | (ReplayBatch::Typed(DataBatch::IndexPrice(_)), Some(DataRef::IndexPrice(_)))
                | (ReplayBatch::Typed(DataBatch::FundingRate(_)), Some(DataRef::FundingRate(_)))
                | (
                    ReplayBatch::Typed(DataBatch::OptionGreeks(_)),
                    Some(DataRef::OptionGreeks(_)),
                )
                | (
                    ReplayBatch::Typed(DataBatch::InstrumentStatus(_)),
                    Some(DataRef::InstrumentStatus(_)),
                )
                | (
                    ReplayBatch::Typed(DataBatch::InstrumentClose(_)),
                    Some(DataRef::InstrumentClose(_)),
                ) => {}
                _ => panic!("data did not use its typed batch: {batch:?}"),
            }
        }
    }

    #[rstest]
    fn test_homogeneous_data_preserves_item_order() {
        let first = QuoteTick {
            ts_init: UnixNanos::from(7),
            ..QuoteTick::default()
        };
        let second = QuoteTick {
            ts_init: UnixNanos::from(3),
            ..QuoteTick::default()
        };

        let batch = ReplayBatch::from_data(vec![Data::Quote(first), Data::Quote(second)]);

        assert_eq!(batch.len(), 2);
        assert!(matches!(
            batch.get(0),
            Some(DataRef::Quote(quote)) if quote.ts_init == UnixNanos::from(7)
        ));
        assert!(matches!(
            batch.get(1),
            Some(DataRef::Quote(quote)) if quote.ts_init == UnixNanos::from(3)
        ));
    }

    #[rstest]
    fn test_mixed_data_remains_one_compatibility_batch() {
        let batch = ReplayBatch::from_data(vec![
            Data::Quote(QuoteTick::default()),
            Data::Deltas(Box::new(stub_deltas())),
            Data::Quote(QuoteTick::default()),
        ]);

        assert!(matches!(batch, ReplayBatch::Compatibility(_)));
        assert_eq!(batch.len(), 3);
        assert!(matches!(batch.get(0), Some(DataRef::Quote(_))));
        assert!(matches!(batch.get(1), Some(DataRef::BookDeltas(_))));
        assert!(matches!(batch.get(2), Some(DataRef::Quote(_))));
    }

    #[rstest]
    fn test_custom_data_remains_compatible_without_cloning() {
        let custom = stub_custom_data(7, 42, None, Some("CUSTOM.SIM".to_string()));
        let payload = Arc::clone(&custom.data);
        let batch = ReplayBatch::from_data(vec![Data::Custom(custom)]);

        assert!(matches!(batch, ReplayBatch::Compatibility(_)));
        assert_eq!(Arc::strong_count(&payload), 2);
        assert!(matches!(batch.get(0), Some(DataRef::Custom(_))));
    }

    #[rstest]
    fn test_deltas_conversion_moves_inner_allocation() {
        let deltas = stub_deltas();
        let deltas_len = deltas.deltas.len();
        let deltas_ptr = deltas.deltas.as_ptr();
        let batch = ReplayBatch::from_data(vec![Data::Deltas(Box::new(deltas))]);
        let Some(DataRef::BookDeltas(converted)) = batch.get(0) else {
            panic!("expected deltas batch item");
        };

        assert!(matches!(
            batch,
            ReplayBatch::Typed(DataBatch::BookDeltas(_))
        ));
        assert_eq!(batch.len(), 1);
        assert_eq!(converted.deltas.len(), deltas_len);
        assert_eq!(converted.deltas.as_ptr(), deltas_ptr);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_defi_data_uses_typed_storage() {
        let instrument_id = InstrumentId::new(Symbol::from("ETH/USDC"), Venue::from("UNISWAPV3"));
        let snapshot = PoolSnapshot::new(
            instrument_id,
            PoolState::default(),
            Vec::new(),
            Vec::new(),
            PoolAnalytics::default(),
            BlockPosition::new(12, "0xabc".to_string(), 3, 4),
            UnixNanos::from(5),
            UnixNanos::from(6),
        );
        let expected = snapshot.clone();
        let batch =
            ReplayBatch::from_data(vec![Data::Defi(Box::new(DefiData::PoolSnapshot(snapshot)))]);

        assert!(matches!(batch, ReplayBatch::Typed(DataBatch::Defi(_))));
        assert_eq!(batch.len(), 1);
        assert!(matches!(
            batch.get(0),
            Some(DataRef::Defi(DefiData::PoolSnapshot(snapshot)))
                if snapshot == &expected
        ));
    }
}

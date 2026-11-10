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

use std::{
    os::raw::c_char,
    sync::{Arc, atomic::AtomicU64},
};

use databento::dbn;
use nautilus_common::enums::Environment;
use nautilus_databento::decode::decode_mbo_msg;
use nautilus_model::{
    data::{Data, NautilusDataType, OrderBookDelta},
    enums::RecordFlag,
    identifiers::InstrumentId,
    types::{PRICE_UNDEF, Price, Quantity},
};
use nautilus_persistence::{
    backend::{default_writer_factories, parquet::catalog::ParquetDataCatalog},
    catalog::factory::CatalogConnectConfig,
    writer::{
        factory::{WriterBackendType, WriterConnectConfig, create_writer},
        feather::WriterClock,
    },
};
use rstest::rstest;
use tempfile::TempDir;

#[rstest]
fn decoded_mbo_clear_promotes_without_changing_values(
    #[values(false, true)] automatic: bool,
    #[values(false, true)] clear_only: bool,
    #[values(false, true)] batched: bool,
) {
    let directory = TempDir::new().unwrap();
    let staging = directory.path().join("backtest/run-mbo");
    let catalog_config = CatalogConnectConfig::new(directory.path().to_string_lossy(), None);
    let mut config = WriterConnectConfig::new(staging.to_string_lossy(), Some(catalog_config));
    config.promote_on_close = automatic;
    let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
    let mut sink = create_writer(
        &WriterBackendType::Parquet,
        &config,
        clock,
        &default_writer_factories(),
    )
    .unwrap();
    let id = InstrumentId::from("ESM4.GLBX");
    let ts_event = 1_700_000_000_000_000_123;
    let ts_init = ts_event + 17;
    let snapshot = dbn::FlagSet::empty().set_snapshot();
    let snapshot_last = dbn::FlagSet::empty().set_snapshot().set_last();
    let mut messages = vec![dbn::MboMsg {
        hd: dbn::RecordHeader::new::<dbn::MboMsg>(dbn::rtype::MBO, 1, 2, ts_event),
        order_id: 0,
        price: dbn::UNDEF_PRICE,
        size: 0,
        flags: if clear_only { snapshot_last } else { snapshot },
        channel_id: 3,
        action: b'R' as c_char,
        side: b'N' as c_char,
        ts_recv: ts_event,
        ts_in_delta: 0,
        sequence: 71,
    }];

    if !clear_only {
        for (side, price, size, order_id, flags) in [
            (b'B', 4_800_250_000_000, 13, 91, snapshot),
            (b'A', 4_801_500_000_000, 29, 92, snapshot_last),
        ] {
            messages.push(dbn::MboMsg {
                action: b'A' as c_char,
                side: side as c_char,
                price,
                size,
                order_id,
                flags,
                ..messages[0]
            });
        }
    }

    let rows = messages
        .iter()
        .map(|message| {
            let (delta, trade) =
                decode_mbo_msg(message, id, 2, Some(ts_init.into()), false).unwrap();
            assert!(trade.is_none());
            delta.unwrap()
        })
        .collect::<Vec<_>>();

    assert_eq!(rows[0].order.price.raw(), PRICE_UNDEF);
    assert_eq!(rows[0].order.price.precision, 0);
    assert_eq!(rows[0].order.size, Quantity::from("0"));
    assert_eq!(rows[0].sequence, 0);
    assert_eq!(
        rows.last().unwrap().flags,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
    );

    if !clear_only {
        assert_eq!(rows[1].order.price, Price::from("4800.25"));
        assert_eq!(rows[2].order.price, Price::from("4801.50"));
    }

    if batched {
        sink.write_batch(rows.iter().copied().map(Data::BookDelta).collect())
            .unwrap();
    } else {
        for row in &rows {
            sink.write_data(Data::BookDelta(*row)).unwrap();
        }
    }

    sink.close().unwrap();
    let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
    if !automatic {
        catalog
            .convert_stream_to_data(
                "run-mbo",
                &NautilusDataType::OrderBookDelta.into(),
                Environment::Backtest,
                None,
                false,
            )
            .unwrap();
    }

    let actual = catalog
        .query::<OrderBookDelta>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&rows).unwrap()
    );

    for (actual, expected) in actual.iter().zip(&rows) {
        assert_eq!(actual.order.price.raw(), expected.order.price.raw());
        assert_eq!(actual.order.price.precision, expected.order.price.precision);
        assert_eq!(actual.order.size.raw(), expected.order.size.raw());
        assert_eq!(actual.order.size.precision, expected.order.size.precision);
    }
}

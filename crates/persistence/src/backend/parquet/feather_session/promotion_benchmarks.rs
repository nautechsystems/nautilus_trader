// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the License
//  is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express
//  or implied. See the License for the specific language governing permissions and limitations under
//  the License.
// -------------------------------------------------------------------------------------------------

//! Isolated wall-clock benchmarks for CLEAR normalization and catalog promotion.
//! Inputs model 64 snapshots with 64 ADDs each, with CLEAR encoded separately.
//! Timings exclude Feather reads, staging metadata restoration, fixture setup, and verification.
//! These tests access private promotion operations without adding a public benchmarking API.
//! Run with `CARGO_BUILD_JOBS=8 cargo test --profile bench -p nautilus-persistence --lib
//! --features high-precision promotion_benchmarks -- --ignored --nocapture --test-threads=1`.

#[cfg(test)]
mod tests {
    use std::{
        hint::black_box,
        time::{Duration, Instant},
    };

    use arrow::record_batch::RecordBatch;
    use nautilus_common::enums::Environment;
    use nautilus_model::{
        data::{BookOrder, NautilusDataType, OrderBookDelta},
        enums::{BookAction, OrderSide},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::EncodeToRecordBatch;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::super::{normalize_clear_precision, split_record_batch_by_identifier};
    use crate::backend::parquet::{
        catalog::ParquetDataCatalog, io::read_parquet_from_object_store,
    };

    #[rstest]
    #[case::snapshot_batches(false, 1, true)]
    #[case::single_rows(true, 1, true)]
    #[case::four_instruments(true, 4, true)]
    #[case::without_clear(true, 1, false)]
    #[ignore = "Wall-clock benchmark; run optimized with one test thread"]
    fn clear_promotion_latency(
        #[case] single_rows: bool,
        #[case] instruments: usize,
        #[case] with_clear: bool,
    ) {
        let batches = snapshot_batches(single_rows, instruments, with_clear);
        let rows = batches
            .iter()
            .flat_map(|batch| split_record_batch_by_identifier(batch).unwrap())
            .collect::<Vec<_>>();
        let row_count = batches.iter().map(RecordBatch::num_rows).sum::<usize>();
        println!(
            "batches={}, rows={row_count}, instruments={instruments}",
            batches.len()
        );

        report_samples("normalize", || {
            let mut elapsed = Duration::ZERO;

            for _ in 0..16 {
                let mut candidate = rows.clone();
                let start = Instant::now();
                normalize_clear_precision(black_box(&mut candidate)).unwrap();
                elapsed += start.elapsed();

                for ((identifier, batch), (original_identifier, original)) in
                    candidate.iter().zip(&rows)
                {
                    assert_eq!(identifier, original_identifier);
                    assert_eq!(batch.columns(), original.columns());
                    assert_eq!(
                        batch.schema().metadata().get("price_precision").unwrap(),
                        "2"
                    );
                    assert_eq!(
                        batch.schema().metadata().get("size_precision").unwrap(),
                        "3"
                    );
                }

                black_box(candidate);
            }

            elapsed / 16
        });

        report_samples("promote", || {
            let directory = TempDir::new().unwrap();
            let catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
            let data_type = NautilusDataType::OrderBookDelta.into();
            let start = Instant::now();
            catalog
                .convert_feather_batches_to_parquet(
                    Environment::Backtest,
                    "bench",
                    &data_type,
                    "backtest/bench/order_book_deltas_0.feather",
                    black_box(&batches),
                    false,
                    Some("bench"),
                )
                .unwrap();
            let elapsed = start.elapsed();
            let files = catalog.get_file_list_from_data_cls(&data_type).unwrap();
            assert_eq!(files.len(), instruments);
            let mut written_rows = 0;

            for file in files {
                let (written, schema) = catalog
                    .execute_async(|| async {
                        read_parquet_from_object_store(
                            catalog.object_store.clone(),
                            &object_store::path::Path::from(file.as_str()),
                        )
                        .await
                    })
                    .unwrap();

                assert_eq!(schema.metadata().get("price_precision").unwrap(), "2");
                assert_eq!(schema.metadata().get("size_precision").unwrap(), "3");
                written_rows += written.iter().map(RecordBatch::num_rows).sum::<usize>();
            }

            assert_eq!(written_rows, row_count);
            elapsed
        });
    }

    fn snapshot_batches(
        single_rows: bool,
        instruments: usize,
        with_clear: bool,
    ) -> Vec<RecordBatch> {
        let mut batches = Vec::new();

        for snapshot in 0..64 {
            let instrument_id = InstrumentId::from(format!("BOOK{}.SIM", snapshot % instruments));
            let timestamp = (snapshot + 1) as u64;

            if with_clear {
                let clear = OrderBookDelta::clear(
                    instrument_id,
                    timestamp,
                    timestamp.into(),
                    timestamp.into(),
                );
                batches.push(OrderBookDelta::encode_batch(&clear.metadata(), &[clear]).unwrap());
            }

            let orders = (0..64)
                .map(|level| {
                    OrderBookDelta::new(
                        instrument_id,
                        BookAction::Add,
                        BookOrder::new(
                            OrderSide::Buy,
                            Price::from("1.23"),
                            Quantity::from("4.500"),
                            level + 1,
                        ),
                        if level == 63 { 128 } else { 0 },
                        timestamp,
                        timestamp.into(),
                        timestamp.into(),
                    )
                })
                .collect::<Vec<_>>();

            if single_rows {
                for order in orders {
                    batches
                        .push(OrderBookDelta::encode_batch(&order.metadata(), &[order]).unwrap());
                }
            } else {
                batches.push(OrderBookDelta::encode_batch(&orders[0].metadata(), &orders).unwrap());
            }
        }

        batches
    }

    fn report_samples(name: &str, mut operation: impl FnMut() -> Duration) {
        for _ in 0..5 {
            black_box(operation());
        }

        let mut samples = (0..21).map(|_| operation()).collect::<Vec<_>>();
        samples.sort_unstable();
        println!(
            "{name}: median={:?}, p10={:?}, p90={:?}, samples={}",
            samples[samples.len() / 2],
            samples[samples.len() / 10],
            samples[samples.len() * 9 / 10],
            samples.len()
        );
    }
}

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

//! Replay catalog adapter backed by Parquet data catalogs.

use nautilus_core::UnixNanos;
use nautilus_model::data::{Bar, QuoteTick, TradeTick};
use nautilus_persistence::backend::catalog::{ParquetDataCatalog, parse_filename_timestamps};

use super::{
    CatalogReplayData, CatalogReplayRecord, CatalogSliceCoverage, CatalogSlicePlan,
    CatalogSliceQuery, ReplayCatalog, ReplayTimeRange,
};

/// Read-only replay catalog adapter backed by [`ParquetDataCatalog`].
#[derive(Debug)]
pub struct ParquetReplayCatalog<'a> {
    catalog: &'a mut ParquetDataCatalog,
}

impl<'a> ParquetReplayCatalog<'a> {
    /// Creates a replay catalog adapter over an existing Parquet catalog.
    pub const fn new(catalog: &'a mut ParquetDataCatalog) -> Self {
        Self { catalog }
    }
}

impl ReplayCatalog for ParquetReplayCatalog<'_> {
    type Error = anyhow::Error;

    fn plan_slice(
        &mut self,
        query: &CatalogSliceQuery,
    ) -> Result<CatalogSliceCoverage, Self::Error> {
        let mut files = self.catalog.query_files(
            &query.data_cls,
            query.identifiers_option(),
            Some(query.start),
            Some(query.end),
        )?;
        files.sort();

        let intervals = files
            .iter()
            .filter_map(|file| {
                parse_filename_timestamps(file).map(|(start, end)| {
                    ReplayTimeRange::new(UnixNanos::from(start), UnixNanos::from(end))
                })
            })
            .collect();

        Ok(CatalogSliceCoverage { files, intervals })
    }

    fn load_slice(
        &mut self,
        plan: &CatalogSlicePlan,
    ) -> Result<Vec<CatalogReplayRecord>, Self::Error> {
        let identifiers = plan.query.identifiers_option();
        let start = Some(plan.query.start);
        let end = Some(plan.query.end);
        let files = Some(plan.coverage.files.clone());

        match plan.query.data_cls.as_str() {
            "quotes" => Ok(catalog_replay_records(
                self.catalog.query_typed_data::<QuoteTick>(
                    identifiers,
                    start,
                    end,
                    None,
                    files,
                    false,
                )?,
            )),
            "trades" => Ok(catalog_replay_records(
                self.catalog.query_typed_data::<TradeTick>(
                    identifiers,
                    start,
                    end,
                    None,
                    files,
                    false,
                )?,
            )),
            "bars" => Ok(catalog_replay_records(
                self.catalog.query_typed_data::<Bar>(
                    identifiers,
                    start,
                    end,
                    None,
                    files,
                    false,
                )?,
            )),
            data_cls => {
                anyhow::bail!("catalog replay loading for {data_cls} is not supported")
            }
        }
    }
}

fn catalog_replay_records<T>(records: Vec<T>) -> Vec<CatalogReplayRecord>
where
    T: Into<CatalogReplayData>,
{
    records
        .into_iter()
        .map(Into::into)
        .map(CatalogReplayRecord::from_data)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        path::Path,
    };

    use nautilus_model::{
        data::{Bar, BarSpecification, BarType, QuoteTick, TradeTick},
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };
    use nautilus_persistence::backend::catalog::{ParquetDataCatalog, timestamps_to_filename};
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn parquet_replay_catalog_plans_selected_slice_files() {
        let temp_dir = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);

        create_catalog_file(temp_dir.path(), "quotes", "AUDUSD.SIM", 1_000, 2_000);
        create_catalog_file(temp_dir.path(), "quotes", "AUDUSD.SIM", 10_000, 11_000);
        create_catalog_file(temp_dir.path(), "quotes", "ETHUSDT.BINANCE", 5_000, 6_000);

        let query = CatalogSliceQuery {
            data_cls: "quotes".to_string(),
            identifiers: vec!["AUD/USD.SIM".to_string()],
            start: UnixNanos::from(1_500),
            end: UnixNanos::from(2_500),
            required: true,
        };
        let coverage = ParquetReplayCatalog::new(&mut catalog)
            .plan_slice(&query)
            .unwrap();

        assert_eq!(coverage.files.len(), 1);
        assert!(
            coverage.files[0].contains("data/quotes/AUDUSD.SIM/"),
            "planned file should come from AUD/USD.SIM partition, was {}",
            coverage.files[0],
        );
        assert_eq!(
            coverage.intervals,
            vec![ReplayTimeRange::new(
                UnixNanos::from(1_000),
                UnixNanos::from(2_000)
            )]
        );

        let full_window_query = CatalogSliceQuery {
            start: UnixNanos::from(0),
            end: UnixNanos::from(12_000),
            ..query.clone()
        };
        let full_window_coverage = ParquetReplayCatalog::new(&mut catalog)
            .plan_slice(&full_window_query)
            .unwrap();

        assert_eq!(full_window_coverage.files.len(), 2);
        assert_eq!(
            full_window_coverage.intervals,
            vec![
                ReplayTimeRange::new(UnixNanos::from(1_000), UnixNanos::from(2_000)),
                ReplayTimeRange::new(UnixNanos::from(10_000), UnixNanos::from(11_000)),
            ]
        );

        let missing_query = CatalogSliceQuery {
            start: UnixNanos::from(20_000),
            end: UnixNanos::from(21_000),
            ..query
        };
        let missing_coverage = ParquetReplayCatalog::new(&mut catalog)
            .plan_slice(&missing_query)
            .unwrap();

        assert!(missing_coverage.is_missing());
        assert!(missing_coverage.intervals.is_empty());
    }

    #[rstest]
    fn parquet_replay_catalog_loads_selected_quote_records() {
        let temp_dir = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let quotes = vec![
            QuoteTick::new(
                instrument_id,
                Price::from("1.0001"),
                Price::from("1.0002"),
                Quantity::from("100"),
                Quantity::from("100"),
                UnixNanos::from(1_000),
                UnixNanos::from(1_000),
            ),
            QuoteTick::new(
                instrument_id,
                Price::from("1.0003"),
                Price::from("1.0004"),
                Quantity::from("200"),
                Quantity::from("200"),
                UnixNanos::from(2_000),
                UnixNanos::from(2_000),
            ),
            QuoteTick::new(
                instrument_id,
                Price::from("1.0005"),
                Price::from("1.0006"),
                Quantity::from("300"),
                Quantity::from("300"),
                UnixNanos::from(3_000),
                UnixNanos::from(3_000),
            ),
        ];
        catalog
            .write_to_parquet(quotes.clone(), None, None, None)
            .expect("write quotes");

        let query = CatalogSliceQuery {
            data_cls: "quotes".to_string(),
            identifiers: vec!["AUD/USD.SIM".to_string()],
            start: UnixNanos::from(1_500),
            end: UnixNanos::from(2_500),
            required: true,
        };
        let mut replay_catalog = ParquetReplayCatalog::new(&mut catalog);
        let coverage = replay_catalog.plan_slice(&query).expect("plan slice");
        let plan = catalog_slice_plan(query, coverage);

        let records = replay_catalog.load_slice(&plan).expect("load slice");

        assert_eq!(
            records,
            vec![CatalogReplayRecord::from_data(CatalogReplayData::Quote(
                quotes[1]
            ))],
        );
    }

    #[rstest]
    fn parquet_replay_catalog_loads_selected_trade_records() {
        let temp_dir = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let trades = vec![
            TradeTick::new(
                instrument_id,
                Price::from("1.0001"),
                Quantity::from("100"),
                AggressorSide::Buyer,
                TradeId::from("T-1"),
                UnixNanos::from(1_000),
                UnixNanos::from(1_000),
            ),
            TradeTick::new(
                instrument_id,
                Price::from("1.0002"),
                Quantity::from("200"),
                AggressorSide::Seller,
                TradeId::from("T-2"),
                UnixNanos::from(2_000),
                UnixNanos::from(2_000),
            ),
            TradeTick::new(
                instrument_id,
                Price::from("1.0003"),
                Quantity::from("300"),
                AggressorSide::Buyer,
                TradeId::from("T-3"),
                UnixNanos::from(3_000),
                UnixNanos::from(3_000),
            ),
        ];
        catalog
            .write_to_parquet(trades.clone(), None, None, None)
            .expect("write trades");

        let query = CatalogSliceQuery {
            data_cls: "trades".to_string(),
            identifiers: vec!["AUD/USD.SIM".to_string()],
            start: UnixNanos::from(1_500),
            end: UnixNanos::from(2_500),
            required: true,
        };
        let mut replay_catalog = ParquetReplayCatalog::new(&mut catalog);
        let coverage = replay_catalog.plan_slice(&query).expect("plan slice");
        let plan = catalog_slice_plan(query, coverage);

        let records = replay_catalog.load_slice(&plan).expect("load slice");

        assert_eq!(
            records,
            vec![CatalogReplayRecord::from_data(CatalogReplayData::Trade(
                trades[1]
            ))],
        );
    }

    #[rstest]
    fn parquet_replay_catalog_loads_selected_bar_records() {
        let temp_dir = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let bars = vec![
            Bar::new(
                bar_type,
                Price::from("1.0000"),
                Price::from("1.0002"),
                Price::from("1.0000"),
                Price::from("1.0001"),
                Quantity::from("100"),
                UnixNanos::from(1_000),
                UnixNanos::from(1_000),
            ),
            Bar::new(
                bar_type,
                Price::from("1.0001"),
                Price::from("1.0004"),
                Price::from("1.0001"),
                Price::from("1.0003"),
                Quantity::from("200"),
                UnixNanos::from(2_000),
                UnixNanos::from(2_000),
            ),
            Bar::new(
                bar_type,
                Price::from("1.0003"),
                Price::from("1.0006"),
                Price::from("1.0003"),
                Price::from("1.0005"),
                Quantity::from("300"),
                UnixNanos::from(3_000),
                UnixNanos::from(3_000),
            ),
        ];
        catalog
            .write_to_parquet(bars.clone(), None, None, None)
            .expect("write bars");

        let query = CatalogSliceQuery {
            data_cls: "bars".to_string(),
            identifiers: vec!["AUD/USD.SIM".to_string()],
            start: UnixNanos::from(1_500),
            end: UnixNanos::from(2_500),
            required: true,
        };
        let mut replay_catalog = ParquetReplayCatalog::new(&mut catalog);
        let coverage = replay_catalog.plan_slice(&query).expect("plan slice");
        let plan = catalog_slice_plan(query, coverage);

        let records = replay_catalog.load_slice(&plan).expect("load slice");

        assert_eq!(
            records,
            vec![CatalogReplayRecord::from_data(CatalogReplayData::Bar(
                bars[1]
            ))],
        );
    }

    #[rstest]
    fn parquet_replay_catalog_rejects_unsupported_load_slice() {
        let temp_dir = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let plan = CatalogSlicePlan {
            query: CatalogSliceQuery {
                data_cls: "order_book_deltas".to_string(),
                identifiers: vec!["AUD/USD.SIM".to_string()],
                start: UnixNanos::from(1_000),
                end: UnixNanos::from(2_000),
                required: true,
            },
            coverage: CatalogSliceCoverage::from_files(vec![
                "data/order_book_deltas/AUDUSD.SIM/1000_2000.parquet".to_string(),
            ]),
        };

        let err = ParquetReplayCatalog::new(&mut catalog)
            .load_slice(&plan)
            .expect_err("unsupported data class must fail");

        assert_eq!(
            err.to_string(),
            "catalog replay loading for order_book_deltas is not supported",
        );
    }

    fn catalog_slice_plan(
        query: CatalogSliceQuery,
        coverage: CatalogSliceCoverage,
    ) -> CatalogSlicePlan {
        CatalogSlicePlan { query, coverage }
    }

    fn create_catalog_file(
        base_path: &Path,
        data_cls: &str,
        identifier: &str,
        start: u64,
        end: u64,
    ) {
        let directory = base_path.join("data").join(data_cls).join(identifier);
        fs::create_dir_all(&directory).unwrap();

        let filename = timestamps_to_filename(UnixNanos::from(start), UnixNanos::from(end));
        File::create(directory.join(filename)).unwrap();
    }
}

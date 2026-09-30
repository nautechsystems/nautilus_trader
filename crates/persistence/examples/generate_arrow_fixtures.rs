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
    fs::File,
    path::{Path, PathBuf},
};

use arrow::record_batch::RecordBatch;
use nautilus_model::data::{Bar, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick};
use nautilus_serialization::arrow::{
    DecodeFromRecordBatch, EncodeToRecordBatch, normalize_legacy_fixed_columns,
};
use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::{Compression, ZstdLevel},
    file::properties::WriterProperties,
};

type RoundTrip = fn(RecordBatch) -> anyhow::Result<RecordBatch>;

const FILES: &[(&str, RoundTrip)] = &[
    ("quotes.parquet", round_trip::<QuoteTick>),
    ("trades.parquet", round_trip::<TradeTick>),
    ("bars.parquet", round_trip::<Bar>),
    ("deltas.parquet", round_trip::<OrderBookDelta>),
    (
        "quotes-3-groups-filter-query.parquet",
        round_trip::<QuoteTick>,
    ),
];

// Current-format fixtures without a legacy source, re-encoded in place
const CURRENT_FILES: &[(&str, RoundTrip)] = &[("depths.parquet", round_trip::<OrderBookDepth>)];

fn main() -> anyhow::Result<()> {
    let nautilus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/nautilus");
    let source_dir = nautilus.join("legacy/64-bit");
    let target_dir = nautilus.join("arrow");

    for (file_name, round_trip) in FILES {
        let source = source_dir.join(file_name);
        let target = target_dir.join(file_name);
        transcode_legacy_fixture(&source, &target, *round_trip)?;
        println!("Wrote {}", target.display());
    }

    for (file_name, round_trip) in CURRENT_FILES {
        let target = target_dir.join(file_name);
        round_trip_fixture(&target, *round_trip)?;
        println!("Wrote {}", target.display());
    }

    Ok(())
}

fn transcode_legacy_fixture(
    source: &Path,
    target: &Path,
    round_trip: RoundTrip,
) -> anyhow::Result<()> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(source)?)?;
    let schema = builder.schema().clone();
    let row_groups = builder.metadata().num_row_groups();
    let preserve_row_groups =
        (0..row_groups).any(|index| builder.metadata().row_group(index).num_rows() != 1);

    let properties = writer_properties();
    let mut writer: Option<ArrowWriter<File>> = None;

    for row_group in 0..row_groups {
        let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(source)?)?
            .with_row_groups(vec![row_group])
            .with_batch_size(65_536)
            .build()?;

        for batch in reader {
            let batch = normalize_legacy_fixed_columns(&batch?.with_schema(schema.clone())?)?;
            let batch = round_trip(batch)?;

            if let Some(writer) = writer.as_mut() {
                writer.write(&batch)?;
            } else {
                let mut created = ArrowWriter::try_new(
                    File::create(target)?,
                    batch.schema(),
                    Some(properties.clone()),
                )?;
                created.write(&batch)?;
                writer = Some(created);
            }
        }

        if preserve_row_groups {
            writer
                .as_mut()
                .ok_or_else(|| {
                    anyhow::anyhow!("legacy fixture {} produced no row groups", source.display())
                })?
                .flush()?;
        }
    }

    writer
        .ok_or_else(|| anyhow::anyhow!("legacy fixture {} produced no batches", source.display()))?
        .close()?;
    Ok(())
}

fn round_trip_fixture(path: &Path, round_trip: RoundTrip) -> anyhow::Result<()> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(path)?)?;
    let schema = builder.schema().clone();
    let batches = builder
        .build()?
        .map(|batch| round_trip(batch?.with_schema(schema.clone())?))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let first = batches
        .first()
        .ok_or_else(|| anyhow::anyhow!("fixture {} has no batches", path.display()))?;
    let mut writer = ArrowWriter::try_new(
        File::create(path)?,
        first.schema(),
        Some(writer_properties()),
    )?;

    for batch in &batches {
        writer.write(batch)?;
    }

    writer.close()?;
    Ok(())
}

fn writer_properties() -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build()
}

// Writes the schema and metadata that the current catalog writer produces for the same values
fn round_trip<T>(batch: RecordBatch) -> anyhow::Result<RecordBatch>
where
    T: DecodeFromRecordBatch + EncodeToRecordBatch,
{
    let metadata = batch.schema().metadata().clone();
    let values = T::decode_batch(&metadata, batch)?;
    let batch = T::encode_batch(&T::chunk_metadata(&values), &values)?;
    let mut batch = batch;
    // Legacy fixtures predate the identifier column
    if let Ok(index) = batch.schema().index_of("identifier") {
        batch.remove_column(index);
    }
    Ok(batch)
}

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

use nautilus_serialization::arrow::normalize_legacy_fixed_columns;
use parquet::{
    arrow::{ARROW_SCHEMA_META_KEY, ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::{Compression, ZstdLevel},
    file::properties::WriterProperties,
};

const FILES: &[&str] = &[
    "quotes.parquet",
    "trades.parquet",
    "bars.parquet",
    "deltas.parquet",
    "quotes-3-groups-filter-query.parquet",
];

fn main() -> anyhow::Result<()> {
    let nautilus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/nautilus");
    let source_dir = nautilus.join("legacy/64-bit");
    let target_dir = nautilus.join("arrow");

    for file_name in FILES {
        let source = source_dir.join(file_name);
        let target = target_dir.join(file_name);
        transcode_legacy_fixture(&source, &target)?;
        println!("Wrote {}", target.display());
    }

    Ok(())
}

fn transcode_legacy_fixture(source: &Path, target: &Path) -> anyhow::Result<()> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(source)?)?;
    let schema = builder.schema().clone();
    let row_groups = builder.metadata().num_row_groups();
    let preserve_row_groups =
        (0..row_groups).any(|index| builder.metadata().row_group(index).num_rows() != 1);
    let metadata = builder
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry.key != ARROW_SCHEMA_META_KEY)
                .cloned()
                .collect()
        });
    let properties = WriterProperties::builder()
        .set_key_value_metadata(metadata)
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();
    let mut writer: Option<ArrowWriter<File>> = None;

    for row_group in 0..row_groups {
        let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(source)?)?
            .with_row_groups(vec![row_group])
            .with_batch_size(65_536)
            .build()?;

        for batch in reader {
            let batch = normalize_legacy_fixed_columns(&batch?.with_schema(schema.clone())?)?;

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

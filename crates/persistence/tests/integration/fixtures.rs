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

use std::{fs::File, path::Path};

use nautilus_serialization::arrow::normalize_legacy_fixed_columns;
use parquet::{
    arrow::{ARROW_SCHEMA_META_KEY, ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    file::properties::WriterProperties,
};
use tempfile::NamedTempFile;

pub(super) fn migrate_market_data_fixture(source: impl AsRef<Path>) -> NamedTempFile {
    let source = source.as_ref();
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(source).unwrap()).unwrap();
    let schema = builder.schema().clone();
    let row_groups = builder.metadata().num_row_groups();
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
        .build();
    let target = tempfile::Builder::new()
        .suffix(".parquet")
        .tempfile()
        .unwrap();
    let mut writer = None;

    for row_group in 0..row_groups {
        let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(source).unwrap())
            .unwrap()
            .with_row_groups(vec![row_group])
            .with_batch_size(65_536)
            .build()
            .unwrap();

        for batch in reader {
            let batch = batch.unwrap().with_schema(schema.clone()).unwrap();
            let batch = normalize_legacy_fixed_columns(&batch).unwrap();
            let writer = writer.get_or_insert_with(|| {
                ArrowWriter::try_new(
                    target.reopen().unwrap(),
                    batch.schema(),
                    Some(properties.clone()),
                )
                .unwrap()
            });
            writer.write(&batch).unwrap();
        }
        writer.as_mut().unwrap().flush().unwrap();
    }

    writer.unwrap().close().unwrap();
    target
}

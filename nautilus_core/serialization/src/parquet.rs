// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{error::Error, fs::File, path::Path};

use arrow::record_batch::RecordBatch;
use parquet::{arrow::ArrowWriter, basic::ZstdLevel, file::properties::WriterProperties};

/// Writes a `RecordBatch` to a Parquet file at the specified `filepath`, with optional compression.
pub fn write_batch_to_parquet(
    batch: &RecordBatch,
    filepath: &Path,
    compression: Option<parquet::basic::Compression>,
) -> Result<(), Box<dyn Error>> {
    // Ensure the parent directory exists
    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(filepath)?;

    // Configure writer properties, defaulting to Zstandard compression if not specified
    let default_compression = parquet::basic::Compression::ZSTD(ZstdLevel::default());
    let writer_props = WriterProperties::builder()
        .set_compression(compression.unwrap_or(default_compression))
        .build();

    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(writer_props))?;
    writer.write(batch)?;
    writer.close()?;

    Ok(())
}

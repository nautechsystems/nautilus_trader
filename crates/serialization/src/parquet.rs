// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{fs, fs::File, path::PathBuf};

use anyhow::{Result, anyhow};
use arrow::record_batch::RecordBatch;
use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    file::{
        properties::WriterProperties,
        reader::{FileReader, SerializedFileReader},
        statistics::Statistics,
    },
};

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.serialization.enums"
    )
)]
pub enum ParquetWriteMode {
    Append = 0,
    Prepend = 1,
    Overwrite = 2,
    NewFile = 3,
}

/// Writes a `RecordBatch` to a Parquet file at the specified `filepath`, with optional compression.
pub fn write_batch_to_parquet(
    batch: RecordBatch,
    filepath: &PathBuf,
    compression: Option<parquet::basic::Compression>,
    write_mode: Option<ParquetWriteMode>,
) -> Result<()> {
    write_batches_to_parquet(&[batch], filepath, compression, None, write_mode)
}

pub fn write_batches_to_parquet(
    batches: &[RecordBatch],
    filepath: &PathBuf,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
    write_mode: Option<ParquetWriteMode>,
) -> Result<()> {
    let used_write_mode = write_mode.unwrap_or(ParquetWriteMode::Overwrite);

    // Ensure the parent directory exists
    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if (used_write_mode == ParquetWriteMode::Append || used_write_mode == ParquetWriteMode::Prepend)
        && filepath.exists()
    {
        // Read existing parquet file
        let file = File::open(filepath)?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let existing_batches: Vec<RecordBatch> = reader.build()?.collect::<Result<Vec<_>, _>>()?;

        if !existing_batches.is_empty() {
            // Get the schema of the existing data.
            let existing_schema = existing_batches[0].schema();

            // Cast new batches to the existing schema.
            let mut cast_batches: Vec<RecordBatch> = Vec::with_capacity(batches.len());

            for batch in batches {
                if batch.schema() != existing_schema {
                    let cast_batch = batch.clone().with_schema(existing_schema.clone())?; // Attempt to cast.
                    cast_batches.push(cast_batch);
                } else {
                    cast_batches.push(batch.clone()); // No cast needed, just clone.
                }
            }

            // Combine batches in the appropriate order
            let combined_batches = if used_write_mode == ParquetWriteMode::Append {
                let mut combined = existing_batches;
                combined.extend(cast_batches);
                combined
            } else {
                // Prepend mode
                let mut combined = Vec::with_capacity(cast_batches.len() + existing_batches.len());
                combined.extend(cast_batches);
                combined.extend(existing_batches);
                combined
            };

            return write_batches_to_file(
                &combined_batches,
                filepath,
                compression,
                max_row_group_size,
            );
        }
    }

    // Default case: create new file or overwrite existing
    write_batches_to_file(batches, filepath, compression, max_row_group_size)
}

pub fn combine_data_files(
    parquet_files: Vec<PathBuf>,
    column_name: &str,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> Result<()> {
    let n_files = parquet_files.len();

    if n_files <= 1 {
        return Ok(());
    }

    // Get min/max for each file
    let min_max_per_file = parquet_files
        .iter()
        .map(|file| min_max_from_parquet_metadata(file, column_name))
        .collect::<Result<Vec<_>, _>>()?;

    // Create ordering by first timestamp
    let mut ordering: Vec<usize> = (0..n_files).collect();
    ordering.sort_by_key(|&i| min_max_per_file[i].0);

    // Check for timestamp intersection
    for i in 1..n_files {
        if min_max_per_file[ordering[i - 1]].1 >= min_max_per_file[ordering[i]].0 {
            return Err(anyhow!(
                "Merging not safe due to intersection of timestamps between files. Aborting."
            ));
        }
    }

    // Create sorted list of files
    let sorted_parquet_files = ordering
        .into_iter()
        .map(|i| parquet_files[i].clone())
        .collect();

    // Combine the files
    combine_parquet_files(sorted_parquet_files, compression, max_row_group_size)
}

pub fn combine_parquet_files(
    file_list: Vec<PathBuf>,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> Result<()> {
    if file_list.len() <= 1 {
        return Ok(());
    }

    // Create readers and immediately build them.  Store the *readers*, not the builders.
    let mut readers = Vec::new();
    for file in &file_list {
        let file = File::open(file)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        readers.push(builder.build()?); // Build immediately and store the reader.
    }

    // Collect all batches into a single vector
    let mut all_batches: Vec<RecordBatch> = Vec::new();
    for reader in &mut readers {
        for batch in reader.by_ref() {
            all_batches.push(batch?);
        }
    }

    // Use write_batches_to_file to write the combined batches
    write_batches_to_file(&all_batches, &file_list[0], compression, max_row_group_size)?;

    // Remove the merged files.
    for file_path in file_list.iter().skip(1) {
        fs::remove_file(file_path)?;
    }

    Ok(())
}

fn write_batches_to_file(
    batches: &[RecordBatch],
    filepath: &PathBuf,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> Result<()> {
    let file = File::create(filepath)?;
    let writer_props = WriterProperties::builder()
        .set_compression(compression.unwrap_or(parquet::basic::Compression::SNAPPY))
        .set_max_row_group_size(max_row_group_size.unwrap_or(5000))
        .build();

    let mut writer = ArrowWriter::try_new(file, batches[0].schema(), Some(writer_props))?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;

    Ok(())
}

pub fn min_max_from_parquet_metadata(file_path: &PathBuf, column_name: &str) -> Result<(i64, i64)> {
    // Open the parquet file
    let file = File::open(file_path)?;
    let reader = SerializedFileReader::new(file)?;

    let metadata = reader.metadata();
    let mut overall_min_value: Option<i64> = None;
    let mut overall_max_value: Option<i64> = None;

    // Iterate through all row groups
    for i in 0..metadata.num_row_groups() {
        let row_group = metadata.row_group(i);

        // Iterate through all columns in this row group
        for j in 0..row_group.num_columns() {
            let col_metadata = row_group.column(j);

            if col_metadata.column_path().string() == column_name {
                if let Some(stats) = col_metadata.statistics() {
                    // Check if we have Int64 statistics
                    if let Statistics::Int64(int64_stats) = stats {
                        // Extract min value if available
                        if let Some(&min_value) = int64_stats.min_opt() {
                            if overall_min_value.is_none() || min_value < overall_min_value.unwrap()
                            {
                                overall_min_value = Some(min_value);
                            }
                        }

                        // Extract max value if available
                        if let Some(&max_value) = int64_stats.max_opt() {
                            if overall_max_value.is_none() || max_value > overall_max_value.unwrap()
                            {
                                overall_max_value = Some(max_value);
                            }
                        }
                    } else {
                        return Err(anyhow!(
                            "Warning: Column name '{column_name}' is not of type i64."
                        ));
                    }
                } else {
                    return Err(anyhow!(
                        "Warning: Statistics not available for column '{column_name}' in row group {i}."
                    ));
                }
            }
        }
    }

    // Return the min/max pair if both are available
    if let (Some(min), Some(max)) = (overall_min_value, overall_max_value) {
        Ok((min, max))
    } else {
        Err(anyhow!(
            "Column '{column_name}' not found or has no Int64 statistics in any row group."
        ))
    }
}

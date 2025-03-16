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

use std::{collections::HashMap, path::PathBuf};

use datafusion::parquet::{
    arrow::ArrowWriter,
    basic::{Compression, ZstdLevel},
    file::properties::WriterProperties,
};
use nautilus_model::data::{Bar, OrderBookDelta, QuoteTick, TradeTick};
use nautilus_persistence::python::backend::session::NautilusDataType;
use nautilus_serialization::arrow::EncodeToRecordBatch;
use serde_json::from_reader;

fn determine_data_type(file_name: &str) -> Option<NautilusDataType> {
    let file_name = file_name.to_lowercase();
    if file_name.contains("quotes") || file_name.contains("quote_tick") {
        Some(NautilusDataType::QuoteTick)
    } else if file_name.contains("trades") || file_name.contains("trade_tick") {
        Some(NautilusDataType::TradeTick)
    } else if file_name.contains("bars") {
        Some(NautilusDataType::Bar)
    } else if file_name.contains("deltas") || file_name.contains("order_book_delta") {
        Some(NautilusDataType::OrderBookDelta)
    } else {
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        return Err("Usage: to_parquet <json_file>".into());
    }
    let file_path = PathBuf::from(&args[1]);

    // Validate file extension
    if !file_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        return Err("Input file must be a json file".into());
    }

    // Determine data type from filename
    let data_type = determine_data_type(file_path.to_str().unwrap())
        .ok_or("Could not determine data type from filename")?;

    // Process based on data type
    match data_type {
        NautilusDataType::QuoteTick => process_data::<QuoteTick>(&file_path)?,
        NautilusDataType::TradeTick => process_data::<TradeTick>(&file_path)?,
        NautilusDataType::Bar => process_data::<Bar>(&file_path)?,
        NautilusDataType::OrderBookDelta => process_data::<OrderBookDelta>(&file_path)?,
        _ => return Err("Unsupported data type".into()),
    }

    Ok(())
}

fn process_data<T>(json_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>>
where
    T: serde::de::DeserializeOwned + EncodeToRecordBatch,
{
    // Setup paths
    let stem = json_path.file_stem().unwrap().to_str().unwrap();
    let parent_path = PathBuf::from(".");
    let parent = json_path.parent().unwrap_or(&parent_path);
    let metadata_path = parent.join(format!("{stem}.metadata.json"));
    let parquet_path = parent.join(format!("{stem}.parquet"));

    // Read JSON data
    let json_data = std::fs::read_to_string(json_path)?;
    let data: Vec<T> = serde_json::from_str(&json_data)?;

    // Read metadata
    let metadata_file = std::fs::File::open(metadata_path)?;
    let metadata: HashMap<String, String> = from_reader(metadata_file)?;

    // Get row group size from metadata
    let rows_per_group: usize = metadata
        .get("rows_per_group")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);

    // Get schema from data type
    let schema = T::get_schema(Some(metadata.clone()));

    // Write to parquet
    let mut output_file = std::fs::File::create(&parquet_path)?;
    {
        let writer_props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::default()))
            .set_max_row_group_size(rows_per_group)
            .build();

        let mut writer = ArrowWriter::try_new(&mut output_file, schema.into(), Some(writer_props))?;

        // Write data in chunks
        for chunk in data.chunks(rows_per_group) {
            let batch = T::encode_batch(&metadata, chunk)?;
            writer.write(&batch)?;
        }
        writer.close()?;
    }

    println!(
        "Successfully wrote {} records to {parquet_path:?}",
        data.len()
    );
    Ok(())
}

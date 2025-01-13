use std::path::PathBuf;

use datafusion::parquet::file::reader::{FileReader, SerializedFileReader};
use nautilus_model::data::Data;
use nautilus_model::data::{to_variant, Bar, OrderBookDelta, QuoteTick, TradeTick};
use nautilus_persistence::backend::session::DataBackendSession;
use nautilus_persistence::python::backend::session::NautilusDataType;
use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};
use serde_json::to_writer_pretty;

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
        return Err("Usage: to_json <file>".into());
    }
    let file_path = PathBuf::from(&args[1]);

    // Validate file extension
    if !file_path
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("parquet"))
    {
        return Err("Input file must be a parquet file".into());
    }

    // Determine data type from filename
    let data_type = determine_data_type(file_path.to_str().unwrap())
        .ok_or("Could not determine data type from filename")?;

    // Setup session and read data
    let mut session = DataBackendSession::new(5000);
    let file_str = file_path.to_str().unwrap();

    // Process based on data type
    match data_type {
        NautilusDataType::QuoteTick => process_data::<QuoteTick>(file_str, &mut session)?,
        NautilusDataType::TradeTick => process_data::<TradeTick>(file_str, &mut session)?,
        NautilusDataType::Bar => process_data::<Bar>(file_str, &mut session)?,
        NautilusDataType::OrderBookDelta => process_data::<OrderBookDelta>(file_str, &mut session)?,
        _ => return Err("Unsupported data type".into()),
    }

    Ok(())
}

fn process_data<T>(
    file_path: &str,
    session: &mut DataBackendSession,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: serde::Serialize + TryFrom<Data> + EncodeToRecordBatch + DecodeDataFromRecordBatch,
{
    // Setup output paths
    let input_path = PathBuf::from(file_path);
    let stem = input_path.file_stem().unwrap().to_str().unwrap();
    let default = PathBuf::from(".");
    let parent = input_path.parent().unwrap_or(&default);
    let json_path = parent.join(format!("{}.json", stem));
    let metadata_path = parent.join(format!("{}.metadata.json", stem));

    // Read parquet metadata
    let parquet_file = std::fs::File::open(file_path)?;
    let reader = SerializedFileReader::new(parquet_file)?;
    let row_group_metadata = reader.metadata().row_group(0);
    let rows_per_group = row_group_metadata.num_rows();

    // Read data
    session.add_file::<T>("data", file_path, None)?;
    let query_result = session.get_query_result();
    let data = query_result.collect::<Vec<_>>();
    let data: Vec<T> = to_variant(data);

    // Extract metadata and add row group info
    let mut metadata = T::chunk_metadata(&data);
    metadata.insert("rows_per_group".to_string(), rows_per_group.to_string());

    // Write data to JSON
    let json_file = std::fs::File::create(json_path)?;
    to_writer_pretty(json_file, &data)?;

    // Write metadata to JSON
    let metadata_file = std::fs::File::create(metadata_path)?;
    to_writer_pretty(metadata_file, &metadata)?;

    println!("Successfully processed {} records", data.len());
    Ok(())
}

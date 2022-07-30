use std::io::Read;

use chrono::NaiveDateTime;

use arrow2::{
    array::Utf8Array,
    datatypes::DataType,
    io::csv::read::{deserialize_column, read_rows, ByteRecord, Reader, ReaderBuilder},
};
use nautilus_core::time::Timestamp;
use nautilus_model::{
    data::tick::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};
use nautilus_persistence::parquet::{EncodeToChunk, ParquetReader, ParquetWriter};

struct CsvReader<R: Read> {
    reader: Reader<R>,
    skip: usize,
}

impl<R> Iterator for CsvReader<R>
where
    R: Read,
{
    type Item = Vec<ByteRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut records = vec![ByteRecord::default(); 1000];
        match read_rows(&mut self.reader, 0, &mut records) {
            Ok(0) | Err(_) => None,
            Ok(rows_read) => {
                self.skip += rows_read;
                Some(records[..rows_read].to_vec())
            }
        }
    }
}

/// Load data from a csv file and write it to a parquet file
/// Use struct specific schema for writing
fn load_data_from_csv(src_file_path: &str, dst_file_path: &str) {
    // create parquet writer
    let mut quote_tick_parquet_writer =
        ParquetWriter::<QuoteTick>::new(dst_file_path, QuoteTick::encode_schema());

    // create csv reader
    let csv_reader = CsvReader {
        reader: ReaderBuilder::new().from_path(src_file_path).unwrap(),
        skip: 0,
    };

    // use predefined constant values for certain field values
    let instrument = InstrumentId::from("EUR/USD.SIM");
    let bid_size = Quantity::from_raw(100_000, 0);
    let ask_size = Quantity::from_raw(100_000, 0);
    // closure to decode a slice of byte records into a vector
    // of quote ticks
    let decode_records_fn = move |byte_records: &[ByteRecord]| -> Vec<QuoteTick> {
        let ts: Vec<Timestamp> = deserialize_column(byte_records, 0, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|ts_val| {
                ts_val.map(|ts_val| {
                    NaiveDateTime::parse_from_str(ts_val, "%Y%m%d %H%M%S%f")
                        .unwrap()
                        .timestamp_nanos() as Timestamp
                })
            })
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let bid: Vec<Price> = deserialize_column(byte_records, 1, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|bid_val| bid_val.map(Price::from))
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let ask: Vec<Price> = deserialize_column(byte_records, 2, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|ask_val| ask_val.map(Price::from))
            .collect::<Option<Vec<_>>>()
            .unwrap();

        // construct iterator of values from field value arrays
        let values = ts
            .into_iter()
            .zip(bid.into_iter())
            .zip(ask.into_iter())
            .map(|((ts, bid), ask)| QuoteTick {
                instrument_id: instrument.clone(),
                bid,
                ask,
                bid_size: bid_size.clone(),
                ask_size: ask_size.clone(),
                ts_event: ts,
                ts_init: ts,
            });

        values.collect()
    };

    let csv_quote_tick = csv_reader.map(|byte_records| decode_records_fn(&byte_records));
    quote_tick_parquet_writer
        .write_bulk(csv_quote_tick)
        .unwrap();
}

/// load data from a parquet file and consume it
fn read_quote_tick_from_parquet(file_path: &str) {
    let pqr: ParquetReader<QuoteTick> = ParquetReader::new(file_path, 10000);
    let mut total = 0;

    for chunk in pqr {
        total += chunk.len();
    }

    println!("{}", total);
}

fn main() {
    load_data_from_csv("../quote_tick_data.csv", "../quote_tick_full.parquet");
    read_quote_tick_from_parquet("../quote_tick_full.parquet");
}

use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use chrono::NaiveDateTime;

use arrow2::{
    array::Utf8Array,
    datatypes::DataType,
    io::csv::read::{deserialize_column, ByteRecord, ReaderBuilder},
};
use nautilus_core::time::Timestamp;
use nautilus_model::{
    data::tick::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};
use nautilus_persistence::parquet::{EncodeToChunk, ParquetReader, ParquetWriter};

/// Load data from a csv file and write it to a parquet file
/// Use struct specific schema for writing
fn load_data_from_csv() {
    let f = File::open("./common/quote_tick_data.csv").unwrap();
    let mut rdr = BufReader::with_capacity(39 * 10000, f);

    let instrument = InstrumentId::from("EUR/USD.SIM");
    let bid_size = Quantity::from_raw(100_000, 0);
    let ask_size = Quantity::from_raw(100_000, 0);
    let mut quote_tick_parquet_writer =
        ParquetWriter::<QuoteTick>::new("quote_tick_full.parquet", QuoteTick::encode_schema());

    loop {
        let mut bytes_read = 0;
        if let Ok(data) = rdr.fill_buf() {
            bytes_read = data.len();
            let mut csv_rdr = ReaderBuilder::new().from_reader(data);
            let records: Vec<ByteRecord> = csv_rdr
                .into_byte_records()
                .filter_map(|rec| rec.ok())
                .collect();
            let ts: Vec<Timestamp> = deserialize_column(&records, 0, DataType::Utf8, 0)
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
            let bid: Vec<Price> = deserialize_column(&records, 1, DataType::Utf8, 0)
                .unwrap()
                .as_any()
                .downcast_ref::<Utf8Array<i32>>()
                .unwrap()
                .into_iter()
                .map(|bid_val| bid_val.map(|bid_val| Price::from(bid_val)))
                .collect::<Option<Vec<_>>>()
                .unwrap();
            let ask: Vec<Price> = deserialize_column(&records, 2, DataType::Utf8, 0)
                .unwrap()
                .as_any()
                .downcast_ref::<Utf8Array<i32>>()
                .unwrap()
                .into_iter()
                .map(|ask_val| ask_val.map(|ask_val| Price::from(ask_val)))
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

            let quote_values: Vec<QuoteTick> = values.collect();
            let _ = quote_tick_parquet_writer.write(quote_values).unwrap();
        } else {
            quote_tick_parquet_writer.end_writer();
            break;
        }

        if bytes_read == 0 {
            quote_tick_parquet_writer.end_writer();
            break;
        } else {
            rdr.consume(bytes_read);
        }
    }
}

/// load data from a parquet file and consume it
fn read_quote_tick_from_parquet() {
    let pqr: ParquetReader<QuoteTick> = ParquetReader::new("quote_tick_full.parquet", 10000);

    for chunk in pqr.into_iter() {
        println!("{}", chunk.len());
    }
}

fn main() {
    load_data_from_csv();
    read_quote_tick_from_parquet();
}

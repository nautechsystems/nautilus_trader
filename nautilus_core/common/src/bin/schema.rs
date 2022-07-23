// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader},
    marker::PhantomData,
    sync::Arc,
};

use chrono::NaiveDateTime;

use arrow2::{
    array::{Array, Int64Array, UInt64Array, Utf8Array},
    chunk::Chunk,
    datatypes::{DataType, Field, Schema},
    error::Result,
    io::{
        csv::read::{deserialize_column, ByteRecord, ReaderBuilder},
        parquet::{
            read::FileReader,
            write::{
                transverse, CompressionOptions, Encoding, FileWriter, RowGroupIterator, Version,
                WriteOptions,
            },
        },
    },
};
use nautilus_core::time::Timestamp;
use nautilus_model::{
    data::tick::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

pub struct ParquetWriter<A> {
    pub writer: FileWriter<File>,
    pub encodings: Vec<Vec<Encoding>>,
    pub options: WriteOptions,
    pub writer_type: PhantomData<*const A>,
}

impl<A> ParquetWriter<A> {
    fn new(path: &str, schema: Schema) -> Self {
        let options = WriteOptions {
            write_statistics: true,
            compression: CompressionOptions::Uncompressed,
            version: Version::V2,
        };

        let encodings = schema
            .fields
            .iter()
            .map(|f| transverse(&f.data_type, |_| Encoding::Plain))
            .collect();

        // Create a new empty file
        let file = File::create(path).unwrap();

        let writer = FileWriter::try_new(file, schema, options).unwrap();

        ParquetWriter {
            writer,
            encodings,
            options,
            writer_type: PhantomData,
        }
    }

    fn write(self: &mut Self, cols: Chunk<Box<dyn Array>>) -> Result<()> {
        let iter = vec![Ok(cols)];
        let row_groups = RowGroupIterator::try_new(
            iter.into_iter(),
            self.writer.schema(),
            self.options,
            self.encodings.clone(),
        )?;

        for group in row_groups {
            self.writer.write(group?)?;
        }
        Ok(())
    }

    fn end_writer(self: &mut Self) {
        let _size = self.writer.end(None);
    }
}

trait DecodeFromChunk
where
    Self: Sized,
{
    fn decode(schema: &Schema, cols: Chunk<Arc<dyn Array>>) -> Vec<Self>;
}

trait EncodeToChunk
where
    Self: Sized,
{
    fn encode_schema() -> Schema;
    fn encode(data: Vec<Self>) -> Chunk<Box<dyn Array>>;
}

impl EncodeToChunk for QuoteTick {
    fn encode_schema() -> Schema {
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let fields = vec![
            Field::new("bid", DataType::Int64, false),
            Field::new("ask", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts", DataType::UInt64, false),
        ];

        let mut metadata = BTreeMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), "8".to_string());
        metadata.insert("qty_precision".to_string(), "0".to_string());
        Schema::from(fields).with_metadata(metadata)
    }

    fn encode(data: Vec<Self>) -> Chunk<Box<dyn Array>> {
        let (mut bid_field, mut ask_field, mut bid_size, mut ask_size, mut ts): (
            Vec<i64>,
            Vec<i64>,
            Vec<u64>,
            Vec<u64>,
            Vec<u64>,
        ) = (vec![], vec![], vec![], vec![], vec![]);

        data.iter().fold((), |(), quote| {
            bid_field.push(quote.bid.raw);
            ask_field.push(quote.ask.raw);
            ask_size.push(quote.ask_size.raw);
            bid_size.push(quote.bid_size.raw);
            ts.push(quote.ts_init);
        });

        let ask_array = Int64Array::from_vec(ask_field);
        let bid_array = Int64Array::from_vec(bid_field);
        let ask_size_array = UInt64Array::from_vec(ask_size);
        let bid_size_array = UInt64Array::from_vec(bid_size);
        let ts_array = UInt64Array::from_vec(ts);
        Chunk::new(vec![
            bid_array.to_boxed(),
            ask_array.to_boxed(),
            ask_size_array.to_boxed(),
            bid_size_array.to_boxed(),
            ts_array.to_boxed(),
        ])
    }
}

impl DecodeFromChunk for QuoteTick {
    fn decode(schema: &Schema, cols: Chunk<Arc<dyn Array>>) -> Vec<Self> {
        let instrument_id = InstrumentId::from(schema.metadata.get("instrument_id").unwrap());
        let price_precision = schema
            .metadata
            .get("price_precision")
            .unwrap()
            .parse::<u8>()
            .unwrap();
        let qty_precision = schema
            .metadata
            .get("qty_precision")
            .unwrap()
            .parse::<u8>()
            .unwrap();

        // extract field value arrays from chunk separately
        let bid_values = cols.arrays()[0]
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let ask_values = cols.arrays()[1]
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let ask_size_values = cols.arrays()[2]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let bid_size_values = cols.arrays()[3]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_values = cols.arrays()[4]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        // construct iterator of values from field value arrays
        let values = bid_values
            .into_iter()
            .zip(ask_values.into_iter())
            .zip(ask_size_values.into_iter())
            .zip(bid_size_values.into_iter())
            .zip(ts_values.into_iter())
            .map(|((((bid, ask), ask_size), bid_size), ts)| QuoteTick {
                instrument_id: instrument_id.clone(),
                bid: Price::from_raw(*bid.unwrap(), price_precision),
                ask: Price::from_raw(*ask.unwrap(), price_precision),
                bid_size: Quantity::from_raw(*bid_size.unwrap(), qty_precision),
                ask_size: Quantity::from_raw(*ask_size.unwrap(), qty_precision),
                ts_event: *ts.unwrap(),
                ts_init: *ts.unwrap(),
            });

        values.collect()
    }
}

struct ParquetReader<'a, A> {
    file_reader: FileReader<&'a File>,
    reader_type: PhantomData<*const A>,
}

impl<'a, A> ParquetReader<'a, A> {
    fn new(f: &'a File, chunk_size: usize) -> Self {
        let fr = FileReader::try_new(f, None, Some(chunk_size), None, None)
            .expect("Unable to create reader from file")
            .into_iter();
        ParquetReader {
            file_reader: fr,
            reader_type: PhantomData,
        }
    }
}

impl<'a, A> Iterator for ParquetReader<'a, A>
where
    A: DecodeFromChunk,
{
    type Item = Vec<A>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(Ok(chunk)) = self.file_reader.next() {
            Some(A::decode(self.file_reader.schema(), chunk))
        } else {
            None
        }
    }
}

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
            let _ = quote_tick_parquet_writer
                .write(QuoteTick::encode(quote_values))
                .unwrap();
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
    let f = File::open("quote_tick_full.parquet").unwrap();
    let pqr: ParquetReader<QuoteTick> = ParquetReader::new(&f, 10000);

    for chunk in pqr.into_iter() {
        println!("{}", chunk.len());
    }
}

fn main() {
    load_data_from_csv();
    read_quote_tick_from_parquet();
}

use itertools::Itertools;
use log::info;
use nautilus_serialization::arrow::{
    bars_to_arrow_record_batch_bytes, order_book_deltas_to_arrow_record_batch_bytes,
    order_book_depth10_to_arrow_record_batch_bytes, quote_ticks_to_arrow_record_batch_bytes,
    trade_ticks_to_arrow_record_batch_bytes,
};
use std::path::PathBuf;

use datafusion::arrow::record_batch::RecordBatch;
use nautilus_model::data::{Data, GetTsInit};
use nautilus_serialization::arrow::EncodingError;
use nautilus_serialization::parquet::write_batches_to_parquet;

pub struct ParquetCatalog {
    base_path: PathBuf,
    batch_size: usize,
}

impl ParquetCatalog {
    pub fn new(base_path: PathBuf, batch_size: Option<usize>) -> Self {
        Self {
            base_path,
            batch_size: batch_size.unwrap_or(5000),
        }
    }

    fn make_path(&self, type_name: &str, instrument_id: Option<&str>) -> PathBuf {
        let mut path = self.base_path.join("data").join(type_name.to_lowercase());

        if let Some(id) = instrument_id {
            path = path.join(id);
        }

        std::fs::create_dir_all(&path).expect("Failed to create directory");
        let file_path = path.join("data.parquet");
        info!("Created directory path: {:?}", file_path);
        file_path
    }

    fn check_ascending_timestamps<T: GetTsInit>(data: &[T], type_name: &str) {
        assert!(
            data.windows(2).all(|w| w[0].ts_init() <= w[1].ts_init()),
            "{} timestamps must be in ascending order",
            type_name
        );
    }

    fn write_batch_to_parquet(batch: RecordBatch, path: PathBuf, type_name: &str) {
        info!("Writing {} data to {:?}", type_name, path);
        write_batches_to_parquet(&[batch], &path, None)
            .expect(&format!("Failed to write {} to parquet", type_name));
    }

    fn write_data<T, F>(
        &self,
        items: Vec<Data>,
        extract_fn: F,
        to_batch_fn: fn(Vec<T>) -> Result<RecordBatch, EncodingError>,
        type_name: &str,
        instrument_id: &str,
    ) where
        F: Fn(Data) -> Option<T>,
        T: GetTsInit,
    {
        let data: Vec<T> = items.into_iter().filter_map(extract_fn).collect();

        ParquetCatalog::check_ascending_timestamps(&data, type_name);

        let path = self.make_path(type_name, Some(instrument_id));
        info!(
            "Processing {} data in chunks of {} to {:?}",
            type_name, self.batch_size, path
        );

        // Convert all chunks to record batches first
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .chunks(self.batch_size)
            .into_iter()
            .map(|chunk| match to_batch_fn(chunk.collect()) {
                Ok(batch) => batch,
                Err(e) => {
                    panic!(
                        "Failed to convert {} chunk to record batch: {:?}",
                        type_name, e
                    )
                }
            })
            .collect();

        // Write all batches to parquet file
        info!(
            "Writing {} batches of {} data to {:?}",
            batches.len(),
            type_name,
            path
        );
        write_batches_to_parquet(&batches, &path, None)
            .expect(&format!("Failed to write {} to parquet", type_name));
    }

    pub fn write_data_enum(&self, data: Vec<Data>) {
        // Group data by variant
        let grouped_data = data.into_iter().chunk_by(|d| std::mem::discriminant(d));

        for (_, group) in grouped_data.into_iter() {
            let items: Vec<_> = group.collect();
            if items.is_empty() {
                continue;
            }

            // Match on the first item to determine the variant type
            match items[0] {
                Data::Delta(ref delta) => {
                    self.write_data(
                        items,
                        |d| {
                            if let Data::Delta(d) = d {
                                Some(d)
                            } else {
                                None
                            }
                        },
                        order_book_deltas_to_arrow_record_batch_bytes,
                        "OrderBookDelta",
                        &delta.instrument_id.to_string(),
                    );
                }
                Data::Depth10(depth) => {
                    self.write_data(
                        items,
                        |d| {
                            if let Data::Depth10(d) = d {
                                Some(d)
                            } else {
                                None
                            }
                        },
                        order_book_depth10_to_arrow_record_batch_bytes,
                        "OrderBookDepth10",
                        &depth.instrument_id.to_string(),
                    );
                }
                Data::Quote(quote) => {
                    self.write_data(
                        items,
                        |d| {
                            if let Data::Quote(d) = d {
                                Some(d)
                            } else {
                                None
                            }
                        },
                        quote_ticks_to_arrow_record_batch_bytes,
                        "QuoteTick",
                        &quote.instrument_id.to_string(),
                    );
                }
                Data::Trade(trade) => {
                    self.write_data(
                        items,
                        |d| {
                            if let Data::Trade(d) = d {
                                Some(d)
                            } else {
                                None
                            }
                        },
                        trade_ticks_to_arrow_record_batch_bytes,
                        "TradeTick",
                        &trade.instrument_id.to_string(),
                    );
                }
                Data::Bar(bar) => {
                    self.write_data(
                        items,
                        |d| if let Data::Bar(d) = d { Some(d) } else { None },
                        bars_to_arrow_record_batch_bytes,
                        "Bar",
                        &bar.bar_type.to_string(),
                    );
                }
                Data::Deltas(_) => {
                    // Handle OrderBookDeltas_API if needed
                    continue;
                }
            }
        }
    }
}

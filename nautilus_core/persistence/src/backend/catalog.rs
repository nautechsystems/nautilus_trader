use itertools::Itertools;
use log::info;
use nautilus_model::data::bar::Bar;
use nautilus_model::data::delta::OrderBookDelta;
use nautilus_model::data::depth::OrderBookDepth10;
use nautilus_model::data::quote::QuoteTick;
use nautilus_model::data::trade::TradeTick;
use nautilus_serialization::arrow::EncodeToRecordBatch;
use std::path::PathBuf;

use datafusion::arrow::record_batch::RecordBatch;
use nautilus_model::data::{Data, GetTsInit};
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

    fn make_path(&self, type_name: &str, instrument_id: Option<&String>) -> PathBuf {
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

    pub fn data_to_record_batches<T>(&self, data: Vec<T>) -> Vec<RecordBatch>
    where
        T: GetTsInit + EncodeToRecordBatch,
    {
        data.into_iter()
            .chunks(self.batch_size)
            .into_iter()
            .map(|chunk| {
                // Take first element and extract metadata
                // SAFETY: Unwrap safe as already checked that `data` not empty
                let data = chunk.collect_vec();
                let metadata = EncodeToRecordBatch::chunk_metadata(&data);
                T::encode_batch(&metadata, &data).expect("Expected to encode batch")
            })
            .collect()
    }

    pub fn write_data<T>(&self, data: Vec<T>, type_name: &str)
    where
        T: GetTsInit + EncodeToRecordBatch,
    {
        ParquetCatalog::check_ascending_timestamps(&data, type_name);

        let batches = self.data_to_record_batches(data);
        if let Some(batch) = batches.first() {
            let schema = batch.schema();
            let instrument_id = schema.metadata.get("instrument_id");
            let path = self.make_path(type_name, instrument_id);

            // Write all batches to parquet file
            info!(
                "Writing {} batches of {} data to {:?}",
                batches.len(),
                type_name,
                path
            );
            write_batches_to_parquet(&batches, &path, None)
                .unwrap_or_else(|_| panic!("Failed to write {} to parquet", type_name));
        }
    }

    pub fn write_data_enum(&self, data: Vec<Data>) {
        let mut delta: Vec<OrderBookDelta> = Vec::new();
        let mut depth10: Vec<OrderBookDepth10> = Vec::new();
        let mut quote: Vec<QuoteTick> = Vec::new();
        let mut trade: Vec<TradeTick> = Vec::new();
        let mut bar: Vec<Bar> = Vec::new();

        for d in data.iter().cloned() {
            match d {
                Data::Delta(d) => {
                    delta.push(d);
                }
                Data::Depth10(d) => {
                    depth10.push(d);
                }
                Data::Quote(d) => {
                    quote.push(d);
                }
                Data::Trade(d) => {
                    trade.push(d);
                }
                Data::Bar(d) => {
                    bar.push(d);
                }
                Data::Deltas(_) => continue,
            }
        }

        self.write_data(delta, "OrderBookDelta");
        self.write_data(depth10, "OrderBookDepth10");
        self.write_data(quote, "QuoteTick");
        self.write_data(trade, "TradeTick");
        self.write_data(bar, "Bar");
    }
}

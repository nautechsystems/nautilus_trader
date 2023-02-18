use datafusion::error::Result;
use datafusion::prelude::*;

use nautilus_model::data::tick::QuoteTick;
use nautilus_persistence::parquet::DecodeFromRecordBatch;
use nautilus_persistence::session::PersistenceSession;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    let reader = PersistenceSession::new();
    let mut parquet_options = ParquetReadOptions::default();
    parquet_options.skip_metadata = Some(false);
    reader
        .register_parquet(
            "quote_tick",
            "../../tests/test_data/quote_tick_data.parquet",
            parquet_options,
        )
        .await?;
    let stream = reader.query("SELECT * FROM quote_tick SORT BY ts_init").await?;

    let metadata: HashMap<String, String> = HashMap::from([
        ("instrument_id".to_string(), "EUR/USD.SIM".to_string()),
        ("price_precision".to_string(), "5".to_string()),
        ("size_precision".to_string(), "0".to_string()),
    ]);

    // extract row batches from stream and decode them to vec of ticks
    let ticks: Vec<QuoteTick> = stream
        .into_iter()
        .flat_map(|batch| {
            dbg!(batch.schema().metadata());
            QuoteTick::decode_batch(&metadata, batch)
        })
        .collect();

    let is_ascending_by_init = |ticks: &Vec<QuoteTick>| {
        for i in 1..ticks.len() {
            if ticks[i].ts_init < ticks[i - 1].ts_init {
                return false;
            }
        }
        true
    };

    assert_eq!("EUR/USD.SIM", ticks[0].instrument_id.to_string());
    assert_eq!(ticks.len(), 9500);
    assert!(is_ascending_by_init(&ticks));
    Ok(())
}

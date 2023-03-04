use datafusion::error::Result;
use datafusion::physical_plan::SendableRecordBatchStream;
use datafusion::prelude::*;
use futures::{stream, Stream, StreamExt};
use nautilus_model::data::tick::QuoteTick;
use nautilus_persistence::parquet::DecodeFromRecordBatch;

use std::pin::Pin;

use stream_kmerge::kmerge_by;

fn batch_stream_to_stream<T>(
    batch_stream: SendableRecordBatchStream,
) -> Pin<Box<dyn Stream<Item = T>>>
where
    T: DecodeFromRecordBatch + 'static,
{
    Box::pin(batch_stream.flat_map(move |result| match result {
        Ok(batch) => {
            let ticks = T::decode_batch(batch.schema().metadata(), batch);
            stream::iter(ticks)
        }
        Err(_err) => panic!("Error result"),
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    let session_ctx = SessionContext::new();
    let parquet_options = ParquetReadOptions::<'_> {
        skip_metadata: Some(false),
        ..Default::default()
    };
    session_ctx
        .register_parquet(
            "quote_tick",
            "../tests/test_data/quote_tick_data.parquet",
            parquet_options,
        )
        .await?;

    let stream1 = session_ctx
        .sql("SELECT * FROM quote_tick ORDER BY ts_init")
        .await?
        .execute_stream()
        .await?;

    let stream2 = session_ctx
        .sql("SELECT * FROM quote_tick ORDER BY ts_init")
        .await?
        .execute_stream()
        .await?;

    let tick_stream = batch_stream_to_stream::<QuoteTick>(stream1);
    let tick2_stream = batch_stream_to_stream::<QuoteTick>(stream2);
    // Note: the underlying implementation is a max heap compare the elements
    // in reverse order to get a minimum element first
    let ticks: Vec<QuoteTick> = kmerge_by(vec![tick_stream, tick2_stream], |tick_1, tick_2| {
        tick_2.ts_init.cmp(&tick_1.ts_init)
    })
    .collect::<Vec<QuoteTick>>()
    .await;

    let is_ascending_by_init = |ticks: &Vec<QuoteTick>| {
        for i in 1..ticks.len() {
            // previous tick is more recent than current tick
            // this is not ascending order
            if ticks[i - 1].ts_init > ticks[i].ts_init {
                return false;
            }
        }
        true
    };

    assert_eq!("EUR/USD.SIM", ticks[0].instrument_id.to_string());
    assert_eq!(ticks.len(), 19000);
    assert!(is_ascending_by_init(&ticks));
    Ok(())
}

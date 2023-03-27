// use nautilus_model::data::tick::QuoteTick;
// use nautilus_persistence::session::{PersistenceCatalog, QueryResult};

#[tokio::main]
async fn main() {
    // let mut catalog = PersistenceCatalog::default();
    // catalog
    //     .add_file(
    //         "quote_tick",
    //         "../../tests/test_data/quote_tick_data.parquet",
    //     )
    //     .await
    //     .unwrap();
    // catalog
    //     .add_file(
    //         "quote_tick_2",
    //         "../../tests/test_data/quote_tick_data.parquet",
    //     )
    //     .await
    //     .unwrap();
    // let query_result: QueryResult<QuoteTick> = catalog.to_query_result();

    // // NOTE: is_sorted_by_key is unstable otherwise use
    // // ticks.is_sorted_by_key(|tick| tick.ts_init)
    // // https://github.com/rust-lang/rust/issues/53485
    // let is_ascending_by_init = |ticks: &Vec<QuoteTick>| {
    //     for i in 1..ticks.len() {
    //         // previous tick is more recent than current tick
    //         // this is not ascending order
    //         if ticks[i - 1].ts_init > ticks[i].ts_init {
    //             return false;
    //         }
    //     }
    //     true
    // };

    // let ticks: Vec<QuoteTick> = query_result.flatten().collect();
    // assert_eq!("EUR/USD.SIM", ticks[0].instrument_id.to_string());
    // assert_eq!(ticks.len(), 19000);
    // assert!(is_ascending_by_init(&ticks));
}

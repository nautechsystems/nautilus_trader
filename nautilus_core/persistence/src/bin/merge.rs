use nautilus_model::data::tick::{Data, QuoteTick};
use nautilus_persistence::session::{PersistenceCatalog, QueryResult};

#[tokio::main]
async fn main() {
    let mut catalog = PersistenceCatalog::default();
    catalog
        .add_file::<QuoteTick>("quote_tick", "../tests/test_data/quote_tick_data.parquet")
        .await
        .unwrap();
    catalog
        .add_file::<QuoteTick>("quote_tick_2", "../tests/test_data/quote_tick_data.parquet")
        .await
        .unwrap();
    let query_result: QueryResult = catalog.to_query_result();

    // NOTE: is_sorted_by_key is unstable otherwise use
    // ticks.is_sorted_by_key(|tick| tick.ts_init)
    // https://github.com/rust-lang/rust/issues/53485
    let is_ascending_by_init = |ticks: &Vec<Data>| {
        for i in 1..ticks.len() {
            // previous tick is more recent than current tick
            // this is not ascending order
            if ticks[i - 1].get_ts_init() > ticks[i].get_ts_init() {
                return false;
            }
        }
        true
    };

    let ticks: Vec<Data> = query_result.flatten().collect();
    match &ticks[0] {
        Data::Trade(_) => unreachable!(),
        Data::Quote(q) => assert_eq!("EUR/USD.SIM", q.instrument_id.to_string()),
    }
    assert_eq!(ticks.len(), 19000);
    assert!(is_ascending_by_init(&ticks));
}

use nautilus_core::UnixNanos;
use nautilus_model::instruments::Instrument;
use nautilus_tardis::{
    enums::TardisExchange,
    http::{client::TardisHttpClient, query::InstrumentFilterBuilder},
};

#[tokio::main]
async fn main() {
    nautilus_common::logging::ensure_logging_initialized();

    let client = TardisHttpClient::new(None, None, None, true).unwrap();

    // Tardis instrument definitions
    let resp = client
        .instruments_info(TardisExchange::Binance, None, None)
        .await;
    println!("Received: {resp:?}");

    let start = UnixNanos::from("2020-1-1");
    let filter = InstrumentFilterBuilder::default()
        .available_since(Some(start.into()))
        .build()
        .unwrap();

    let resp = client
        .instruments_info(TardisExchange::Binance, Some("BTCUSDT"), Some(&filter))
        .await;
    println!("Received: {resp:?}");

    let filter = InstrumentFilterBuilder::default()
        .instrument_type(Some(vec!["perpetual".to_string()]))
        .build()
        .unwrap();
    let resp = client
        .instruments_info(TardisExchange::Bitmex, Some("XBTUSD"), Some(&filter))
        .await;

    for inst in resp.unwrap() {
        println!("{inst:?}");
        if let Some(changes) = inst.changes {
            for change in changes {
                println!("Change:");
                println!("{change:?}");
            }
        }
    }

    let effective = UnixNanos::from("2020-08-01");

    // Nautilus instrument definitions
    let resp = client
        .instruments(
            TardisExchange::Bitmex,
            Some("XBTUSD"),
            Some(&filter),
            None,
            None,
            None,
            Some(effective),
            None,
        )
        .await;

    for inst in resp.unwrap() {
        println!("{}", inst.id());
        println!("price_increment={}", inst.price_increment());
        println!("size_increment={}", inst.size_increment());
        println!("multiplier={}", inst.multiplier());
        println!("ts_event={}", inst.ts_event().to_rfc3339());
        println!("ts_init={}", inst.ts_init().to_rfc3339());
        println!("---------------------------");
    }
}

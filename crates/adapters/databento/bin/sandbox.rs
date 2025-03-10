use std::env;

use databento::{
    LiveClient,
    dbn::{Dataset::GlbxMdp3, MboMsg, SType, Schema, TradeMsg},
    live::Subscription,
};
use time::OffsetDateTime;

#[tokio::main]
async fn main() {
    let mut client = LiveClient::builder()
        .key(env::var("DATABENTO_API_KEY").unwrap())
        .unwrap()
        .dataset(GlbxMdp3)
        .build()
        .await
        .unwrap();

    client
        .subscribe(
            Subscription::builder()
                .schema(Schema::Mbo)
                .stype_in(SType::RawSymbol)
                .symbols("ESM4")
                .start(OffsetDateTime::from_unix_timestamp_nanos(0).unwrap())
                .build(),
        )
        .await
        .unwrap();

    client.start().await.unwrap();

    let mut count = 0;

    while let Some(record) = client.next_record().await.unwrap() {
        if let Some(msg) = record.get::<TradeMsg>() {
            println!("{msg:#?}");
        }
        if let Some(msg) = record.get::<MboMsg>() {
            println!(
                "Received delta: {} {} flags={}",
                count,
                msg.hd.ts_event,
                msg.flags.raw(),
            );
            count += 1;
        }
    }
}

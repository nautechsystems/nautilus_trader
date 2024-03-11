use std::env;

use databento::{
    dbn::{Dataset::GlbxMdp3, SType, Schema},
    live::Subscription,
    LiveClient,
};
use dbn::TradeMsg;

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
            &Subscription::builder()
                .schema(Schema::Trades)
                .stype_in(SType::RawSymbol)
                .symbols("ESM4")
                .build(),
        )
        .await
        .unwrap();

    client.start().await.unwrap();

    while let Some(record) = client.next_record().await.unwrap() {
        if let Some(trade) = record.get::<TradeMsg>() {
            println!("{trade:#?}");
        }
    }
}

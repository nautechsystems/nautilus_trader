use std::path::Path;

use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() {
    // Specify the CSV filepath
    let filepath = Path::new("YOUR_CSV_DATA_PATH");

    // Optionally specify one or both precisions
    let price_precision = Some(1);
    let size_precision = Some(0);

    // Optionally specify an instrument ID and/or limit
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let limit = None;

    // Consider propagating any parsing error depending on your workflow
    let _deltas = nautilus_tardis::csv::load_deltas(
        filepath,
        price_precision,
        size_precision,
        Some(instrument_id),
        limit,
    )
    .unwrap();
}

use nautilus_bin::strategy::grid_mm::{config::GridMarketMakerConfig, strategy::GridMarketMaker};
use nautilus_bin::exchange::Exchange;
use nautilus_model::identifiers::TraderId;
use nautilus_model::types::Quantity;

const EXCHANGE_STR: &str = "bybit";

const TRADER_ID: &str = "TESTER-001";

const MAX_POSITION: &str = "20";
const TRADE_SIZE: &str = "20";
const NUM_LEVELS: usize = 1;
const GRID_STEP_BPS: u32 = 15;
const SKEW_FACTOR: f64 = 0.1;
const REQUOTE_THRESHOLD_BPS: u32 = 5;
const EXPIRE_TIME_SECS: u64 = 5;
const ON_CANCEL_RESUBMIT: bool = true;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    // tracing_subscriber::fmt::init();
        nautilus_common::logging::ensure_logging_initialized(); 
        
    let exchange: Exchange = EXCHANGE_STR.parse()?;
    let trader_id = TraderId::from(TRADER_ID);

    let (mut node, instrument_id) = exchange.build_node(trader_id)?;

    let config = GridMarketMakerConfig::builder()
        .instrument_id(instrument_id)
        .max_position(Quantity::from(MAX_POSITION))
        .trade_size(Quantity::from(TRADE_SIZE))
        .num_levels(NUM_LEVELS)
        .grid_step_bps(GRID_STEP_BPS)
        .skew_factor(SKEW_FACTOR)
        .requote_threshold_bps(REQUOTE_THRESHOLD_BPS)
        .expire_time_secs(EXPIRE_TIME_SECS)
        .on_cancel_resubmit(ON_CANCEL_RESUBMIT)
        .build();
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

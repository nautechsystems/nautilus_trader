//! `mmm` - mattia's market maker

use nautilus_backtest::node::BacktestNode;
use nautilus_bin::config::Config;
use nautilus_bin::exchange::Exchange;
use nautilus_bin::strategy::mmm::config::MattiasMarketMakerConfig;
use nautilus_bin::strategy::mmm::strategy::MattiasMarketMaker;
use nautilus_common::enums::Environment;
use nautilus_model::{
    enums::{AccountType, OmsType}, identifiers::{InstrumentId, TraderId},
};

use nautilus_backtest::config::{
    BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType,
};
use nautilus_model::enums::BookType;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let config = Config::load("config.toml".to_string())?.mmm.unwrap();
    let parquet_path = config.path.clone();

    let exchange: Exchange = config.exchange.parse()?;
    let trader_id = TraderId::from(config.trader_id.as_str());

    let mut node = exchange.build_node(trader_id)?;
    let instrument_id = InstrumentId::from(config.instrument_id);

    let mmm_config = MattiasMarketMakerConfig::builder()
        .instrument_id(instrument_id)
        .path(parquet_path.clone())
        .build();

    let strategy = MattiasMarketMaker::new(&mmm_config);



    match &config.execution_environment {
        Environment::Backtest => {
            let venue = BacktestVenueConfig::builder()
                .name("BYBIT")
                .oms_type(OmsType::Hedging)
                .account_type(AccountType::Margin)
                .book_type(BookType::L2_MBP)
                .starting_balances(vec!["1_000 USDT".to_string()])
                .build()?;

            let order_book = BacktestDataConfig::builder()
                .catalog_path(parquet_path.clone())
                .instrument_id(instrument_id)
                .data_type(NautilusDataType::OrderBookDelta)
                .build()?;

            let trades = BacktestDataConfig::builder()
                .catalog_path(parquet_path.clone())
                .instrument_id(instrument_id)
                .data_type(NautilusDataType::TradeTick)
                .build()?;

            let run = BacktestRunConfig::builder()
                .id("mmm-backtest".to_string())
                .venues(vec![venue])
                .data(vec![order_book, trades])
                .chunk_size(100_000)
                .build()?;

            let mut node = BacktestNode::new(vec![run])?;

            node.build()?;

            let engine = node.get_engine_mut("mmm-backtest").unwrap();

            let strategy = MattiasMarketMaker::new(&mmm_config);

            engine.add_strategy(strategy)?;

            node.run()?;
        }
        Environment::Sandbox => todo!(),
        Environment::Live => todo!(),
    }

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

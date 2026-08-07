#![allow(mixed_script_confusables)]

//! `mmm` - mattia's market maker

use nautilus_backtest::node::BacktestNode;
use nautilus_bin::config::Config;
use nautilus_bin::exchange::Exchange;
use nautilus_bin::strategy::mmm::config::MattiasMarketMakerConfig;
use nautilus_bin::strategy::mmm::strategy::MattiasMarketMaker;
use nautilus_common::enums::Environment;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, InstrumentId, TraderId},
};

use nautilus_backtest::config::{
    BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType,
};
use nautilus_model::enums::BookType;

use chrono::{TimeZone, Utc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let config = Config::load("/etc/nautilus-trader/config.toml".to_string())?
        .mmm
        .unwrap();

    log::info!("{config:#?}");

    let parquet_path = config.path.clone();

    let exchange: Exchange = config.exchange.parse()?;
    let trader_id = TraderId::from(config.trader_id.as_str());

    let mut node = exchange.build_node(trader_id)?;
    let instrument_id = InstrumentId::from(config.instrument_id);

    let mmm_config = MattiasMarketMakerConfig::builder()
        .instrument_id(instrument_id)
        .catalog_path(parquet_path.clone())
        .Φ_n(config.Φ_n)
        .Φ_0(config.Φ_0)
        .Q_max(config.Q_max)
        .Δ_μ(config.Δ_μ)
        .Δ_0(config.Δ_0)
        .β(config.β)
        .build();

    let strategy = MattiasMarketMaker::new(&mmm_config);

    let start_date = Utc.with_ymd_and_hms(2026, 8, 2, 0, 0, 0).single().unwrap();
    let end_date = Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).single().unwrap();

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
                .start_time(start_date.into())
                .end_time(end_date.into())
                .data_type(NautilusDataType::OrderBookDelta)
                .optimize_file_loading(true)
                .build()?;

            let trades = BacktestDataConfig::builder()
                .catalog_path(parquet_path.clone())
                .instrument_id(instrument_id)
                .start_time(start_date.into())
                .end_time(end_date.into())
                .data_type(NautilusDataType::TradeTick)
                .optimize_file_loading(true)
                .build()?;

            let run = BacktestRunConfig::builder()
                .id("mmm-backtest".to_string())
                .venues(vec![venue])
                .data(vec![order_book, trades])
                .chunk_size(1_000_000)
                .build()?;

            let mut node = BacktestNode::new(vec![run])?;

            node.build()?;
            {
                let engine = node.get_engine_mut("mmm-backtest").unwrap();

                let strategy = MattiasMarketMaker::new(&mmm_config);

                engine.add_strategy(strategy)?;
            }
            node.run()?;

            let engine = node.get_engine_mut("mmm-backtest").unwrap();

            let snapshots = engine
                .kernel()
                .portfolio
                .borrow()
                .snapshots(&AccountId::from("BYBIT-001"));

            log::info!("{snapshots:#?}");
        }
        Environment::Sandbox => todo!(),
        Environment::Live => {
            node.add_strategy(strategy)?;
            node.run().await?;
        }
    }

    Ok(())
}

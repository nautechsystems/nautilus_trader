use anyhow::Ok;
use nautilus_common::actor::DataActor;
use nautilus_model::{
    enums::BookType::L2_MBP, identifiers::InstrumentId, instruments::InstrumentAny,
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{StrategyCore, nautilus_strategy};
use std::{fmt::Debug, num::NonZeroUsize, path::Path, time::Duration};

use crate::{strategy::mmm::config::MattiasMarketMakerConfig, utils::micro_price};

pub struct MattiasMarketMaker {
    pub core: StrategyCore,
    pub instrument_id: InstrumentId,
    pub instrument_precision: u8,
    pub data_catalog: ParquetDataCatalog,
}

impl MattiasMarketMaker {
    pub fn new(config: &MattiasMarketMakerConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            data_catalog: ParquetDataCatalog::new(Path::new(&config.path), None, None, None, None),
            instrument_id: config.instrument_id,
            instrument_precision: 0,
        }
    }
}

nautilus_strategy!(MattiasMarketMaker, {});

impl Debug for MattiasMarketMaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder").finish()
    }
}

impl DataActor for MattiasMarketMaker {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_trades(self.instrument_id, None, None);

        self.subscribe_book_deltas(
            self.instrument_id,
            L2_MBP,
            NonZeroUsize::new(50),
            None,
            true,
            None,
        );

        self.clock().set_timer(
            "MMM_TIMER",
            Duration::from_millis(100),
            None,
            None,
            None,
            None,
            None,
        )?;

        let instrument = self.cache().instrument(&self.instrument_id).unwrap();
        log::info!("{instrument:#?}");
        match instrument {
            InstrumentAny::Betting(_betting_instrumentt) => todo!(),
            InstrumentAny::BinaryOption(_binary_option) => todo!(),
            InstrumentAny::Cfd(_cfd) => todo!(),
            InstrumentAny::Commodity(_commodity) => todo!(),
            InstrumentAny::CryptoFuture(_crypto_future) => todo!(),
            InstrumentAny::CryptoFuturesSpread(_crypto_futures_spread) => todo!(),
            InstrumentAny::CryptoOption(_crypto_option) => todo!(),
            InstrumentAny::CryptoOptionSpread(_crypto_option_spread) => todo!(),
            InstrumentAny::CryptoPerpetual(crypto_perpetual) => {
                self.instrument_precision = crypto_perpetual.price_precision;
            }
            InstrumentAny::CurrencyPair(_currency_pair) => todo!(),
            InstrumentAny::Equity(_equity) => todo!(),
            InstrumentAny::FuturesContract(_futures_contract) => todo!(),
            InstrumentAny::FuturesSpread(_futures_spread) => todo!(),
            InstrumentAny::IndexInstrument(_index_instrument) => todo!(),
            InstrumentAny::OptionContract(_option_contract) => todo!(),
            InstrumentAny::OptionSpread(_option_spread) => todo!(),
            InstrumentAny::PerpetualContract(_perpetual_contract) => todo!(),
            InstrumentAny::TokenizedAsset(_tokenized_asset) => todo!(),
        }

        Ok(())
    }

    fn on_time_event(&mut self, event: &nautilus_common::timer::TimeEvent) -> anyhow::Result<()> {
        log::info!("hello from mmm {event:#?}");

        if let Some(order_book) = self.cache().order_book(&self.instrument_id) {
            if let (Some(best_bid_price), Some(best_ask_price)) =
                (order_book.best_bid_price(), order_book.best_ask_price())
            {
                let best_bid_size = order_book.best_bid_size().unwrap_or_default();
                let best_ask_size = order_book.best_ask_size().unwrap_or_default();

                let micro_price = micro_price(
                    best_ask_price,
                    best_bid_price,
                    best_ask_size,
                    best_bid_size,
                    self.instrument_precision,
                );
                log::info!("μprice {micro_price}");
            } else {
                log::warn!(
                    "OrderBook trovato nella cache, ma è vuoto (nessun livello Bid/Ask popolare ancora)."
                );
            }
        } else {
            log::error!(
                "OrderBook NON TROVATO nella cache per l'instrument_id: {:?}",
                self.instrument_id
            );
        }

        Ok(())
    }

    fn on_book_deltas(
        &mut self,
        _deltas: &nautilus_model::data::OrderBookDeltas,
    ) -> anyhow::Result<()> {
        // self.cache().order_book(deltas)?;
        Ok(())
    }
}

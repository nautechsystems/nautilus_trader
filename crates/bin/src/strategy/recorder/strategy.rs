use std::{fmt::Debug, num::NonZeroUsize, path::Path, time::Duration};

use nautilus_common::actor::DataActor;

use nautilus_model::{
    data::Data,
    enums::BookType::L2_MBP,
    identifiers::InstrumentId,
    instruments::InstrumentAny,
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{StrategyCore, nautilus_strategy};

use crate::strategy::recorder::config::RecorderConfig;
pub struct Recorder {
    pub(super) core: StrategyCore,
    pub(crate) instrument_id: InstrumentId,
    pub(crate) instrument_precision: u8,
    pub(crate) data_catalog: ParquetDataCatalog,
    pub(crate) buffer: Vec<Data>,
    pub(crate) is_first_timer: bool
}

impl Recorder {
    #[allow(dead_code)]
    pub fn new(config: RecorderConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            instrument_id: config.instrument_id,
            instrument_precision: 0,
            data_catalog: ParquetDataCatalog::new(Path::new(&config.path), None, None, None, None),
            buffer: Vec::with_capacity(100_000),
            is_first_timer: true,
        }
    }
}
nautilus_strategy!(Recorder, {});

impl Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder").finish()
    }
}

fn micro_price(
    ask_price: Price,
    bid_price: Price,
    ask_size: Quantity,
    bid_size: Quantity,
    precision: u8,
) -> Price {
    Price::new(
        ((bid_size.as_decimal() * ask_price.as_decimal()
            + ask_size.as_decimal() * bid_price.as_decimal())
            / (bid_size + ask_size).as_decimal())
        .as_f64(),
        precision,
    )
}

impl DataActor for Recorder {
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!("{:#?}", self.instrument_id);

        // self.subscribe_quotes(self.instrument, None, None);
        // self.subscribe_book_deltas(self.instrument, L1_MBP, None, None, false, None);

        self.subscribe_trades(self.instrument_id, None, None);

        // self.subscribe_book_at_interval(
        //     self.instrument_id,
        //     L2_MBP,
        //     Some(NonZeroUsize::new(1).unwrap()),
        //     NonZeroUsize::new(100).unwrap(),
        //     None,
        //     None,
        // );

        self.subscribe_book_deltas(
            self.instrument_id,
            L2_MBP,
            NonZeroUsize::new(50),
            None,
            false,
            None,
        );

        self.clock().set_timer(
            "RECORDER_TIMER",
            Duration::from_mins(1),
            None,
            None,
            None,
            None,
            None,
        )?;

        let instrument = self.cache().instrument(&self.instrument_id).unwrap();
        log::info!("{:#?}", instrument);
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
                self.instrument_precision = crypto_perpetual.price_precision
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
        // self.instrument_precision = instrument.

        Ok(())
    }

    // fn on_quote(&mut self, quote: &nautilus_model::data::QuoteTick) -> anyhow::Result<()> {
    //     log::debug!("{:#?}", quote);
    //     Ok(())
    // }

    fn on_book(&mut self, order_book: &nautilus_model::orderbook::OrderBook) -> anyhow::Result<()> {
        // log::info!("{:?}", order_book);

        let ask_price = order_book.best_ask_price().unwrap();
        let bid_price = order_book.best_bid_price().unwrap();
        let ask_size = order_book.best_ask_size().unwrap();
        let bid_size = order_book.best_bid_size().unwrap();

        // self.instrument.pri
        let _micro_price = micro_price(
            ask_price,
            bid_price,
            ask_size,
            bid_size,
            self.instrument_precision,
        );

        // log::info!("μprice {:?}", micro_price);
        // self.buffer.push(Data::(order_book));
        Ok(())
    }

    fn on_book_deltas(
        &mut self,
        deltas: &nautilus_model::data::OrderBookDeltas,
    ) -> anyhow::Result<()> {
        log::debug!("{:#?}", deltas);
        deltas.deltas.iter().for_each(|delta| {
            self.buffer.push(Data::Delta(*delta));
        });

        Ok(())
    }

    fn on_trade(&mut self, tick: &nautilus_model::data::TradeTick) -> anyhow::Result<()> {

        log::debug!("{:?}", tick);

        self.buffer.push(Data::Trade(*tick));
        Ok(())
    }

    fn on_time_event(&mut self, event: &nautilus_common::timer::TimeEvent) -> anyhow::Result<()> {
        log::debug!("{:?}", event);
        log::debug!("{:?}", self.data_catalog);

        if self.is_first_timer {
            self.buffer.remove(0);
            self.is_first_timer = false;
        }
        


        self.data_catalog
            .write_data_enum(&self.buffer, None, None, None)?;

        log::info!("data wrote to catalog. lines -> {}", self.buffer.len());

        self.buffer.clear();

        Ok(())
    }
}

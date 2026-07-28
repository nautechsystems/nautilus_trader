use std::{fmt::Debug, num::NonZeroUsize, path::Path, time::Duration};

use ahash::{HashMap, HashMapExt};
use nautilus_common::actor::DataActor;

use nautilus_model::{
    data::Data, enums::BookType::L2_MBP, identifiers::InstrumentId, instruments::InstrumentAny,
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{StrategyCore, nautilus_strategy};

use crate::strategy::recorder::config::RecorderConfig;
pub struct Recorder {
    pub core: StrategyCore,
    pub instrument_id: Vec<InstrumentId>,
    pub instrument_precision: HashMap<InstrumentId, u8>,
    pub data_catalog: ParquetDataCatalog,
    pub buffer: HashMap<InstrumentId, Vec<Data>>,
    pub is_first_timer: bool,
    pub book_depth: usize,
    pub interval_parquet_dump_seconds: u64,
}

impl Recorder {
    #[allow(dead_code)]
    pub fn new(config: &RecorderConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            instrument_id: config.instrument_id.clone(),
            instrument_precision: HashMap::new(),
            data_catalog: ParquetDataCatalog::new(Path::new(&config.path), None, None, None, None),
            buffer: HashMap::new(),
            is_first_timer: true,
            book_depth: config.book_depth,
            interval_parquet_dump_seconds: config.interval_parquet_dump_seconds,
        }
    }
}
nautilus_strategy!(Recorder, {});

impl Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder").finish()
    }
}

impl DataActor for Recorder {
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!("{:#?}", self.instrument_id);

        self.clock().set_timer(
            "RECORDER_TIMER",
            Duration::from_secs(self.interval_parquet_dump_seconds),
            None,
            None,
            None,
            None,
            None,
        )?;

        self.instrument_id.clone().iter().for_each(|instrument_id| {
            self.subscribe_trades(*instrument_id, None, None);

            self.subscribe_book_deltas(
                *instrument_id,
                L2_MBP,
                NonZeroUsize::new(self.book_depth),
                None,
                false,
                None,
            );

            let instrument = self.cache().instrument(instrument_id).unwrap();

            self.data_catalog
                .write_instruments(vec![instrument.clone()])
                .unwrap();

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
                    self.instrument_precision
                        .insert(*instrument_id, crypto_perpetual.price_precision);
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

            self.buffer.insert(*instrument_id, Vec::with_capacity(100_000));
        });
        Ok(())
    }

    fn on_book_deltas(
        &mut self,
        deltas: &nautilus_model::data::OrderBookDeltas,
    ) -> anyhow::Result<()> {
        log::debug!("{deltas:#?}");
        // deltas.instrument_id
        deltas.deltas.iter().for_each(|delta| {
            self.buffer
                .get_mut(&deltas.instrument_id)
                .unwrap()
                .push(Data::Delta(*delta));
        });

        Ok(())
    }

    fn on_trade(&mut self, tick: &nautilus_model::data::TradeTick) -> anyhow::Result<()> {
        log::debug!("{tick:?}");

        self.buffer
            .get_mut(&tick.instrument_id)
            .unwrap()
            .push(Data::Trade(*tick));
        Ok(())
    }

    fn on_time_event(&mut self, event: &nautilus_common::timer::TimeEvent) -> anyhow::Result<()> {
        log::info!("{event:?}");
        log::info!("{:?}", self.data_catalog);

        if self.is_first_timer {
            self.buffer.iter_mut().for_each(|(_, value)| {
                value.remove(0);
            });
            self.is_first_timer = false;
        }

        self.buffer.iter_mut().for_each(|(key, value)| {
            self.data_catalog
                .write_data_enum(value, None, None, None)
                .unwrap();

            log::info!(
                "{} data wrote to catalog. lines -> {}",
                key,
                value.len()
            );

            value.clear();
        });

        Ok(())
    }
}

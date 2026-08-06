use anyhow::Ok;
use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{BookType::L2_MBP, OrderSide::Sell, TimeInForce::Gtd},
    identifiers::InstrumentId,
    instruments::InstrumentAny,
    types::{Currency, Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{Strategy, StrategyCore, nautilus_strategy};
use rust_decimal::Decimal;
use std::{fmt::Debug, num::NonZeroUsize, path::Path, str::FromStr, time::Duration};

use crate::strategy::mmm::{
    config::MattiasMarketMakerConfig,
    strategy::formulas::{r, λ, μ},
};

#[allow(non_snake_case)]
pub mod formulas {
    use nautilus_model::types::{Price, Quantity};
    use rust_decimal::Decimal;

    pub fn μ(
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

    pub fn r(μ: Price, q: Quantity, λ: Decimal, precision: u8) -> Price {
        let r = μ.as_decimal() - q.as_decimal() * λ;
        Price::new(r.as_f64(), precision)
    }

    pub fn λ(average_spread: Decimal, maximum_inventory: Decimal) -> Decimal {
        average_spread / maximum_inventory
    }

    pub fn Δ(Δ_0: Decimal, β: Decimal, σ: Decimal) -> Decimal {
        Δ_0 + β * σ
    }

    pub fn ask_bid_price(r: Price, Δ: Decimal, precision: u8) -> (Price, Price) {
        let ask_price = r.as_decimal() + Δ / Decimal::new(2, 0);
        let bid_price = r.as_decimal() - Δ / Decimal::new(2, 0);
        (
            Price::new(ask_price.as_f64(), precision),
            Price::new(bid_price.as_f64(), precision),
        )
    }
}

#[allow(non_snake_case)]
pub struct MattiasMarketMaker {
    pub core: StrategyCore,
    pub instrument_id: InstrumentId,
    pub instrument_precision: u8,
    pub data_catalog: ParquetDataCatalog,
    pub Φ_n: u8,
    pub Φ_0: Quantity,
    pub Q_max: Quantity,
}

impl MattiasMarketMaker {
    pub fn new(config: &MattiasMarketMakerConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            data_catalog: ParquetDataCatalog::new(
                Path::new(&config.catalog_path),
                None,
                None,
                None,
                None,
            ),
            instrument_id: config.instrument_id,
            instrument_precision: 0,
            Φ_n: 0,
            Φ_0: Quantity::zero(0),
            Q_max: Quantity::zero(0),
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
            Duration::from_mins(10),
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
        let order_lifetime = Duration::from_secs(5).as_nanos() as u64;
        let expire_time = UnixNanos::from(event.ts_event.as_u64() + order_lifetime);

        let venue = self.instrument_id.venue;

        let total_balance = self
            .cache()
            .account_for_venue(&venue)
            .unwrap()
            .balance(Some(Currency::USDT()))
            .unwrap()
            .total;

        let order_book = self.cache().order_book(&self.instrument_id).unwrap();

        let best_bid_price = order_book.best_bid_price().unwrap();
        let best_ask_price = order_book.best_ask_price().unwrap();

        let best_bid_size = order_book.best_bid_size().unwrap();
        let best_ask_size = order_book.best_ask_size().unwrap();

        let order_size = total_balance.as_decimal() * self.Φ_0.as_decimal();

        let open_orders =
            self.cache()
                .orders_open(Some(&venue), Some(&self.instrument_id), None, None, None);

        let q = self
            .cache()
            .positions_open(Some(&venue), Some(&self.instrument_id), None, None, None)
            .iter()
            .map(|position| position.quantity)
            .sum();

        let μ = μ(
            best_ask_price,
            best_bid_price,
            best_ask_size,
            best_bid_size,
            self.instrument_precision,
        );

        let λ = λ(Decimal::from_str("0.01").unwrap(), self.Q_max.as_decimal());

        let reservation_price = r(μ, q, λ, self.instrument_precision);
        let order = self.order().limit(
            self.instrument_id,
            Sell,
            Quantity::from_decimal(order_size).unwrap(),
            Price::from_str("100.0").unwrap(),
            Some(Gtd),
            Some(expire_time),
            Some(true),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        self.submit_order(order, None, None, None).unwrap();

        // if let Some(order_book) = self.cache().order_book(&self.instrument_id) {
        //     if let (Some(best_bid_price), Some(best_ask_price)) =
        //         (order_book.best_bid_price(), order_book.best_ask_price())
        //     {
        //         let best_bid_size = order_book.best_bid_size().unwrap_or_default();
        //         let best_ask_size = order_book.best_ask_size().unwrap_or_default();

        //         let _micro_price = μ(
        //             best_ask_price,
        //             best_bid_price,
        //             best_ask_size,
        //             best_bid_size,
        //             self.instrument_precision,
        //         );
        //         // log::info!("μprice {micro_price}");
        //     } else {
        //         log::warn!(
        //             "OrderBook trovato nella cache, ma è vuoto (nessun livello Bid/Ask popolare ancora)."
        //         );
        //     }
        // } else {
        //     log::error!(
        //         "OrderBook NON TROVATO nella cache per l'instrument_id: {:?}",
        //         self.instrument_id
        //     );
        // }

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

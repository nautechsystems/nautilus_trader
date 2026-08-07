use anyhow::Ok;
use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{
        BookType::L2_MBP,
        OrderSide::{Buy, Sell},
        PositionSide::{Flat, Long, NoPositionSide, Short},
        TimeInForce::Gtd,
    },
    events::OrderFilled,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{Strategy, StrategyCore, nautilus_strategy};
use rust_decimal::Decimal;
use std::{fmt::Debug, num::NonZeroUsize, path::Path, time::Duration};

use crate::strategy::mmm::{
    config::MattiasMarketMakerConfig,
    strategy::formulas::{ask_bid_price, r, Δ, λ, μ},
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

    pub fn r(μ: Price, q: Decimal, λ: Decimal, precision: u8) -> Price {
        let r = μ.as_decimal() - q * λ;
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
    pub Δ_0: Decimal,
    pub β: Decimal,
    pub Δ_μ: Decimal,
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
            Φ_n: config.Φ_n,
            Φ_0: config.Φ_0,
            Q_max: config.Q_max,
            Δ_0: config.Δ_0,
            β: config.β,
            Δ_μ: config.Δ_μ,
        }
    }
}

nautilus_strategy!(MattiasMarketMaker, {
    fn on_order_filled(&mut self, event: &OrderFilled) {
        log::error!("{event:#?}");
    }
});

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
            Duration::from_secs(5),
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

    #[allow(non_snake_case)]
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

        // let open_orders =
        //     self.cache()
        //         .orders_open(Some(&venue), Some(&self.instrument_id), None, None, None);

        let instrument_size_precision = self
            .cache()
            .instrument(&self.instrument_id)
            .unwrap()
            .size_precision();

        let q = self
            .cache()
            .positions_open(Some(&venue), Some(&self.instrument_id), None, None, None)
            .iter()
            .map(|position| match position.side {
                NoPositionSide | Flat => Decimal::ZERO,
                Long => position.quantity.as_decimal(),
                Short => -position.quantity.as_decimal(),
            })
            .sum();

        log::warn!("q:\t\t\t{q:#?}");

        let μ = μ(
            best_ask_price,
            best_bid_price,
            best_ask_size,
            best_bid_size,
            self.instrument_precision,
        );

        let λ = λ(self.Δ_μ, self.Q_max.as_decimal());
        let reservation_price = r(μ, q, λ, self.instrument_precision);
        let Δ = Δ(self.Δ_0, self.β, Decimal::ONE);

        log::warn!("reservation_price:\t{reservation_price:#?}");
        log::warn!("Δ:\t\t\t{Δ:#?}");

        // let tick = self.cache().instrument(&self.instrument_id).unwrap().price_increment();

        let (ask_price, bid_price) = ask_bid_price(reservation_price, Δ, self.instrument_precision);

        // if bid_price <= best_ask_price {
        //     bid_price = best_bid_price - tick;
        // }
        // if ask_price >= best_bid_price {
        //     ask_price = best_ask_price + tick;
        // }

        let sell_order = self.order().limit(
            self.instrument_id,
            Sell,
            Quantity::from_decimal_dp(order_size, instrument_size_precision).unwrap(),
            ask_price,
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

        let buy_order = self.order().limit(
            self.instrument_id,
            Buy,
            Quantity::from_decimal_dp(order_size, instrument_size_precision).unwrap(),
            bid_price,
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

        let sell_order_submit = self.submit_order(sell_order, None, None, None);
        let buy_order_submit = self.submit_order(buy_order, None, None, None);

        if let Err(error) = sell_order_submit {
            log::error!("{error:#?}");
        }

        if let Err(error) = buy_order_submit {
            log::error!("{error:#?}");
        }
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

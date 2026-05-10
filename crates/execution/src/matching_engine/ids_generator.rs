// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_common::cache::Cache;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::OmsType,
    identifiers::{PositionId, TradeId, Venue, VenueOrderId},
    orders::{Order, OrderAny},
};

// FNV-1a 64-bit constants (see http://www.isthe.com/chongo/tech/comp/fnv/).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

pub struct IdsGenerator {
    venue: Venue,
    raw_id: u32,
    oms_type: OmsType,
    use_random_ids: bool,
    use_position_ids: bool,
    cache: Rc<RefCell<Cache>>,
    position_count: usize,
    order_count: usize,
    execution_count: usize,
}

impl Debug for IdsGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(IdsGenerator))
            .field("venue", &self.venue)
            .field("raw_id", &self.raw_id)
            .finish()
    }
}

impl IdsGenerator {
    pub const fn new(
        venue: Venue,
        oms_type: OmsType,
        raw_id: u32,
        use_random_ids: bool,
        use_position_ids: bool,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        Self {
            venue,
            raw_id,
            oms_type,
            cache,
            use_random_ids,
            use_position_ids,
            position_count: 0,
            order_count: 0,
            execution_count: 0,
        }
    }

    pub const fn reset(&mut self) {
        self.position_count = 0;
        self.order_count = 0;
        self.execution_count = 0;
    }

    /// Retrieves or generates a unique venue order ID for the given order.
    ///
    /// # Errors
    ///
    /// Returns an error if ID generation fails.
    pub fn get_venue_order_id(&mut self, order: &OrderAny) -> anyhow::Result<VenueOrderId> {
        // check existing on order
        if let Some(venue_order_id) = order.venue_order_id() {
            return Ok(venue_order_id);
        }

        // check existing in cache
        if let Some(venue_order_id) = self.cache.borrow().venue_order_id(&order.client_order_id()) {
            return Ok(venue_order_id.to_owned());
        }

        let venue_order_id = self.generate_venue_order_id();
        self.cache.borrow_mut().add_venue_order_id(
            &order.client_order_id(),
            &venue_order_id,
            false,
        )?;
        Ok(venue_order_id)
    }

    /// Retrieves or generates a position ID for the given order.
    ///
    /// # Panics
    ///
    /// Panics if `generate` is `Some(true)` but no cached position ID is available.
    pub fn get_position_id(
        &mut self,
        order: &OrderAny,
        generate: Option<bool>,
    ) -> Option<PositionId> {
        let generate = generate.unwrap_or(true);

        if self.oms_type == OmsType::Hedging {
            {
                let cache = self.cache.as_ref().borrow();
                let position_id_result = cache.position_id(&order.client_order_id());
                if let Some(position_id) = position_id_result {
                    return Some(position_id.to_owned());
                }
            }

            if generate {
                self.generate_venue_position_id()
            } else {
                panic!(
                    "Position id should be generated. Hedging Oms type order matching engine doesn't exist in cache."
                )
            }
        } else {
            // Netting OMS (position id will be derived from instrument and strategy)
            let cache = self.cache.as_ref().borrow();
            let positions_open =
                cache.positions_open(None, Some(&order.instrument_id()), None, None, None);
            if positions_open.is_empty() {
                None
            } else {
                Some(positions_open[0].id)
            }
        }
    }

    pub fn generate_trade_id(&mut self, ts_init: UnixNanos) -> TradeId {
        self.execution_count += 1;
        // Trade IDs are always deterministic; `use_random_ids` only affects
        // venue order IDs and position IDs. A bounded FNV-1a hash of
        // `(venue, raw_id, ts_init)` keeps the ID under the 36-character
        // `TradeId` cap for arbitrary-length venue names; `ts_init` protects
        // against collisions after `reset()` rewinds `execution_count`, and
        // the trailing counter distinguishes multiple fills at the same ts.
        let hash = fnv1a_trade_id_hash(self.venue, self.raw_id, ts_init.as_u64());
        let trade_id = format!("T-{hash:016x}-{:03}", self.execution_count);
        TradeId::from(trade_id.as_str())
    }

    pub fn generate_venue_position_id(&mut self) -> Option<PositionId> {
        if !self.use_position_ids {
            return None;
        }

        self.position_count += 1;

        if self.use_random_ids {
            Some(PositionId::new(UUID4::new().to_string()))
        } else {
            Some(PositionId::new(
                format!("{}-{}-{}", self.venue, self.raw_id, self.position_count).as_str(),
            ))
        }
    }

    pub fn generate_venue_order_id(&mut self) -> VenueOrderId {
        self.order_count += 1;

        if self.use_random_ids {
            VenueOrderId::new(UUID4::new().to_string())
        } else {
            VenueOrderId::new(
                format!("{}-{}-{}", self.venue, self.raw_id, self.order_count).as_str(),
            )
        }
    }
}

fn fnv1a_trade_id_hash(venue: Venue, raw_id: u32, ts_init_ns: u64) -> u64 {
    let mut hash: u64 = FNV_OFFSET_BASIS;

    for bytes in [
        venue.as_str().as_bytes(),
        b"\x1f",
        &raw_id.to_le_bytes(),
        b"\x1f",
        &ts_init_ns.to_le_bytes(),
    ] {
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::cache::Cache;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OmsType, OrderSide, OrderType},
        events::OrderFilled,
        identifiers::{
            AccountId, ClientOrderId, PositionId, TradeId, Venue, VenueOrderId, stubs::account_id,
        },
        instruments::{
            CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt,
        },
        orders::{Order, OrderAny, OrderTestBuilder},
        position::Position,
        types::{Price, Quantity},
    };
    use rstest::{fixture, rstest};

    use crate::matching_engine::ids_generator::IdsGenerator;

    #[fixture]
    fn instrument_eth_usdt(crypto_perpetual_ethusdt: CryptoPerpetual) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt)
    }

    #[fixture]
    fn market_order_buy(instrument_eth_usdt: InstrumentAny) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .client_order_id(ClientOrderId::from("O-19700101-000000-001-001-1"))
            .submit(true)
            .build()
    }

    #[fixture]
    fn market_order_sell(instrument_eth_usdt: InstrumentAny) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.000"))
            .client_order_id(ClientOrderId::from("O-19700101-000000-001-001-2"))
            .submit(true)
            .build()
    }

    #[fixture]
    fn market_order_fill(
        instrument_eth_usdt: InstrumentAny,
        account_id: AccountId,
        market_order_buy: OrderAny,
    ) -> OrderFilled {
        OrderFilled::new(
            market_order_buy.trader_id(),
            market_order_buy.strategy_id(),
            market_order_buy.instrument_id(),
            market_order_buy.client_order_id(),
            VenueOrderId::new("BINANCE-1"),
            account_id,
            TradeId::new("1"),
            market_order_buy.order_side(),
            market_order_buy.order_type(),
            Quantity::from("1"),
            Price::from("1000.000"),
            instrument_eth_usdt.quote_currency(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("P-1")),
            None,
        )
    }

    fn get_ids_generator(
        cache: Rc<RefCell<Cache>>,
        use_position_ids: bool,
        oms_type: OmsType,
    ) -> IdsGenerator {
        IdsGenerator::new(
            Venue::from("BINANCE"),
            oms_type,
            1,
            false,
            use_position_ids,
            cache,
        )
    }

    #[rstest]
    fn test_get_position_id_hedging_with_existing_position(
        instrument_eth_usdt: InstrumentAny,
        market_order_buy: OrderAny,
        market_order_fill: OrderFilled,
    ) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut ids_generator = get_ids_generator(cache.clone(), false, OmsType::Hedging);

        let position = Position::new(&instrument_eth_usdt, market_order_fill);

        // Add position to cache
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Hedging)
            .unwrap();

        let position_id = ids_generator.get_position_id(&market_order_buy, None);
        assert_eq!(position_id, Some(position.id));
    }

    #[rstest]
    fn test_get_position_id_hedging_with_generated_position(market_order_buy: OrderAny) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut ids_generator = get_ids_generator(cache, true, OmsType::Hedging);

        let position_id = ids_generator.get_position_id(&market_order_buy, None);
        assert_eq!(position_id, Some(PositionId::new("BINANCE-1-1")));
    }

    #[rstest]
    fn test_get_position_id_netting(
        instrument_eth_usdt: InstrumentAny,
        market_order_buy: OrderAny,
        market_order_fill: OrderFilled,
    ) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut ids_generator = get_ids_generator(cache.clone(), false, OmsType::Netting);

        // position id should be none in non-initialized position id for this instrument
        let position_id = ids_generator.get_position_id(&market_order_buy, None);
        assert_eq!(position_id, None);

        // create and add position in cache
        let position = Position::new(&instrument_eth_usdt, market_order_fill);
        cache
            .as_ref()
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        // position id should be returned for the existing position
        let position_id = ids_generator.get_position_id(&market_order_buy, None);
        assert_eq!(position_id, Some(position.id));
    }

    #[rstest]
    fn test_generate_venue_position_id() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut ids_generator_with_position_ids =
            get_ids_generator(cache.clone(), true, OmsType::Netting);
        let mut ids_generator_no_position_ids = get_ids_generator(cache, false, OmsType::Netting);

        assert_eq!(
            ids_generator_no_position_ids.generate_venue_position_id(),
            None
        );

        let position_id_1 = ids_generator_with_position_ids.generate_venue_position_id();
        let position_id_2 = ids_generator_with_position_ids.generate_venue_position_id();
        assert_eq!(position_id_1, Some(PositionId::new("BINANCE-1-1")));
        assert_eq!(position_id_2, Some(PositionId::new("BINANCE-1-2")));
    }

    #[rstest]
    fn test_generate_venue_position_id_random_uses_uuid4_seam() {
        // Pin that the use_random_ids branch routes through the UUID4 seam
        // (RFC 4122 v4) rather than a raw uuid::Uuid::new_v4 call. The seam
        // already swaps to madsim::rand::thread_rng() under cfg(madsim).
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut generator = IdsGenerator::new(
            Venue::from("BINANCE"),
            OmsType::Netting,
            1,
            true,
            true,
            cache,
        );

        let id = generator.generate_venue_position_id().expect("position id");
        let s = id.as_str();

        assert_eq!(s.len(), 36, "expected canonical UUID4 length");
        assert_eq!(s.as_bytes()[14], b'4', "expected UUID v4 version digit");
        assert!(
            matches!(s.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
            "expected RFC 4122 variant byte",
        );
    }

    #[rstest]
    fn get_venue_position_id(market_order_buy: OrderAny, market_order_sell: OrderAny) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut ids_generator = get_ids_generator(cache, true, OmsType::Netting);

        let venue_order_id1 = ids_generator.get_venue_order_id(&market_order_buy).unwrap();
        let venue_order_id2 = ids_generator
            .get_venue_order_id(&market_order_sell)
            .unwrap();
        assert_eq!(venue_order_id1, VenueOrderId::from("BINANCE-1-1"));
        assert_eq!(venue_order_id2, VenueOrderId::from("BINANCE-1-2"));

        // check if venue order id is cached again
        let venue_order_id3 = ids_generator.get_venue_order_id(&market_order_buy).unwrap();
        assert_eq!(venue_order_id3, VenueOrderId::from("BINANCE-1-1"));
    }

    fn build_ids_generator(venue: Venue, raw_id: u32) -> IdsGenerator {
        let cache = Rc::new(RefCell::new(Cache::default()));
        IdsGenerator::new(venue, OmsType::Netting, raw_id, false, true, cache)
    }

    #[rstest]
    fn test_generate_trade_id_format_and_length_bound() {
        let mut generator =
            build_ids_generator(Venue::from("SOMETHING_VERY_LONG_FOR_SAFETY"), 4_294_967_295);
        let ts = UnixNanos::from(u64::MAX);

        let trade_id = generator.generate_trade_id(ts);
        let value = trade_id.as_str();

        assert!(value.len() <= 36);
        assert!(value.starts_with("T-"));
        assert_eq!(value.len(), "T-0123456789abcdef-001".len());
    }

    #[rstest]
    fn test_generate_trade_id_is_deterministic_across_reset_for_same_ts() {
        let mut generator = build_ids_generator(Venue::from("BINANCE"), 1);
        let ts = UnixNanos::from(1_700_000_000_000_000_000_u64);

        let first = generator.generate_trade_id(ts);
        generator.reset();
        let second = generator.generate_trade_id(ts);
        assert_eq!(
            first, second,
            "same ts_init and reset execution_count must reproduce the same id"
        );
    }

    #[rstest]
    fn test_generate_trade_id_differs_when_ts_init_changes() {
        let mut generator = build_ids_generator(Venue::from("BINANCE"), 1);
        let ts = UnixNanos::from(1_700_000_000_000_000_000_u64);

        let first = generator.generate_trade_id(ts);
        generator.reset();
        let second = generator.generate_trade_id(ts + UnixNanos::from(1));
        assert_ne!(
            first, second,
            "distinct ts_init must produce distinct ids across a reset"
        );
    }

    #[rstest]
    fn test_generate_trade_id_counter_tiebreaker_for_same_ts() {
        let mut generator = build_ids_generator(Venue::from("BINANCE"), 1);
        let ts = UnixNanos::from(1_700_000_000_000_000_000_u64);

        let first = generator.generate_trade_id(ts);
        let second = generator.generate_trade_id(ts);
        let third = generator.generate_trade_id(ts);
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert!(first.as_str().ends_with("-001"));
        assert!(second.as_str().ends_with("-002"));
        assert!(third.as_str().ends_with("-003"));
    }

    #[rstest]
    fn test_generate_trade_id_differs_when_venue_or_raw_id_changes() {
        let ts = UnixNanos::from(1_700_000_000_000_000_000_u64);

        let mut gen_a = build_ids_generator(Venue::from("BINANCE"), 1);
        let mut gen_b = build_ids_generator(Venue::from("BYBIT"), 1);
        let mut gen_c = build_ids_generator(Venue::from("BINANCE"), 2);

        let a = gen_a.generate_trade_id(ts);
        let b = gen_b.generate_trade_id(ts);
        let c = gen_c.generate_trade_id(ts);
        assert_ne!(a, b, "venue must distinguish ids");
        assert_ne!(a, c, "raw_id must distinguish ids");
    }

    // Parity fixtures: if either Rust or Python changes the hashing scheme,
    // one of these assertions will fail and flag the drift.
    // The Python mirror lives at python/tests/unit/backtest/test_trade_id_parity.py
    #[rstest]
    #[case::zero("BINANCE", 1_u32, 0_u64, "T-59d6cf33c843f0cc-001")]
    #[case::nanos(
        "BINANCE",
        1_u32,
        1_700_000_000_000_000_000_u64,
        "T-5c080ffb681dc0d4-001"
    )]
    #[case::long_venue(
        "SOMETHING_VERY_LONG_FOR_SAFETY",
        42_u32,
        1_700_000_000_000_000_000_u64,
        "T-2a2238c5cc0cbaf2-001"
    )]
    fn test_generate_trade_id_matches_python_parity_fixture(
        #[case] venue: &str,
        #[case] raw_id: u32,
        #[case] ts_init: u64,
        #[case] expected: &str,
    ) {
        let mut generator = build_ids_generator(Venue::from(venue), raw_id);
        let trade_id = generator.generate_trade_id(UnixNanos::from(ts_init));
        assert_eq!(trade_id.as_str(), expected);
    }

    // Multi-tick parity: four consecutive bumps at the same ts_init (the
    // bar O/H/L/C pattern) must produce counters 001..004. Mirrored in
    // tests/unit_tests/backtest/test_trade_id_parity.py
    // (test_trade_id_multi_tick_counter_matches_rust_parity_fixture).
    #[rstest]
    fn test_generate_trade_id_multi_tick_matches_python_parity_fixture() {
        let mut generator = build_ids_generator(Venue::from("BINANCE"), 1);
        let ts = UnixNanos::from(1_700_000_000_000_000_000_u64);
        let sequence: Vec<String> = (0..4)
            .map(|_| generator.generate_trade_id(ts).as_str().to_string())
            .collect();

        assert_eq!(
            sequence,
            vec![
                "T-5c080ffb681dc0d4-001".to_string(),
                "T-5c080ffb681dc0d4-002".to_string(),
                "T-5c080ffb681dc0d4-003".to_string(),
                "T-5c080ffb681dc0d4-004".to_string(),
            ],
        );
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::cache::Cache;
use nautilus_model::{
    enums::OmsType,
    identifiers::{PositionId, TradeId, Venue, VenueOrderId},
    orders::{Order, OrderAny},
};
use uuid::Uuid;

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
                    "Position id should be generated. Hedging Oms type order matching engine doesnt exists in cache."
                )
            }
        } else {
            // Netting OMS (position id will be derived from instrument and strategy)
            let cache = self.cache.as_ref().borrow();
            let positions_open =
                cache.positions_open(None, Some(&order.instrument_id()), None, None);
            if positions_open.is_empty() {
                None
            } else {
                Some(positions_open[0].id)
            }
        }
    }

    pub fn generate_trade_id(&mut self) -> TradeId {
        self.execution_count += 1;
        let trade_id = if self.use_random_ids {
            Uuid::new_v4().to_string()
        } else {
            format!("{}-{}-{}", self.venue, self.raw_id, self.execution_count)
        };
        TradeId::from(trade_id.as_str())
    }

    pub fn generate_venue_position_id(&mut self) -> Option<PositionId> {
        if !self.use_position_ids {
            return None;
        }

        self.position_count += 1;
        if self.use_random_ids {
            Some(PositionId::new(Uuid::new_v4().to_string()))
        } else {
            Some(PositionId::new(
                format!("{}-{}-{}", self.venue, self.raw_id, self.position_count).as_str(),
            ))
        }
    }

    pub fn generate_venue_order_id(&mut self) -> VenueOrderId {
        self.order_count += 1;
        if self.use_random_ids {
            VenueOrderId::new(Uuid::new_v4().to_string())
        } else {
            VenueOrderId::new(
                format!("{}-{}-{}", self.venue, self.raw_id, self.order_count).as_str(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::cache::Cache;
    use nautilus_model::{
        enums::OmsType,
        events::OrderFilled,
        identifiers::{PositionId, Venue, VenueOrderId},
        instruments::InstrumentAny,
        orders::OrderAny,
        position::Position,
    };
    use rstest::rstest;

    use crate::matching_engine::{
        ids_generator::IdsGenerator,
        tests::{instrument_eth_usdt, market_order_buy, market_order_fill, market_order_sell},
    };

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
            .add_position(position.clone(), OmsType::Hedging)
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
            .add_position(position.clone(), OmsType::Netting)
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
}

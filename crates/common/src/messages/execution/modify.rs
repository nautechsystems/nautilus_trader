use std::fmt::Display;

use derive_builder::Builder;
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
pub struct ModifyOrder {
    pub trader_id: TraderId,
    pub client_id: Option<ClientId>,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub quantity: Option<Quantity>,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl ModifyOrder {
    /// Creates a new [`ModifyOrder`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            quantity,
            price,
            trigger_price,
            command_id,
            ts_init,
            params,
        }
    }
}

impl Display for ModifyOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ModifyOrder(instrument_id={}, client_order_id={}, venue_order_id={:?}, quantity={}, price={}, trigger_price={})",
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.quantity
                .map_or("None".to_string(), |quantity| format!("{quantity}")),
            self.price
                .map_or("None".to_string(), |price| format!("{price}")),
            self.trigger_price
                .map_or("None".to_string(), |trigger_price| format!(
                    "{trigger_price}"
                )),
        )
    }
}

#[cfg(test)]
mod tests {}

//! Order management handler traits and implementations.
//!
//! These handlers enable the [`OrderManager`](super::manager::OrderManager) to dispatch order commands to
//! components like the [`OrderEmulator`] for processing emulated orders.

use nautilus_common::messages::execution::SubmitOrder;
use nautilus_core::WeakCell;
use nautilus_model::{orders::OrderAny, types::Quantity};

use crate::order_emulator::emulator::OrderEmulator;

pub trait SubmitOrderHandler {
    fn handle_submit_order(&self, command: SubmitOrder);
}

/// Uses [`WeakCell`] to avoid circular references between components.
#[derive(Clone, Debug)]
pub enum SubmitOrderHandlerAny {
    OrderEmulator(WeakCell<OrderEmulator>),
}

impl SubmitOrderHandler for SubmitOrderHandlerAny {
    fn handle_submit_order(&self, command: SubmitOrder) {
        match self {
            Self::OrderEmulator(emulator_weak) => {
                if let Some(emulator) = emulator_weak.upgrade() {
                    emulator.borrow_mut().handle_submit_order(command);
                }
            }
        }
    }
}

pub trait CancelOrderHandler {
    fn handle_cancel_order(&self, order: &OrderAny);
}

/// Uses [`WeakCell`] to avoid circular references between components.
#[derive(Clone, Debug)]
pub enum CancelOrderHandlerAny {
    OrderEmulator(WeakCell<OrderEmulator>),
}

impl CancelOrderHandler for CancelOrderHandlerAny {
    fn handle_cancel_order(&self, order: &OrderAny) {
        match self {
            Self::OrderEmulator(emulator_weak) => {
                if let Some(emulator) = emulator_weak.upgrade() {
                    emulator.borrow_mut().cancel_order(order);
                }
            }
        }
    }
}

pub trait ModifyOrderHandler {
    fn handle_modify_order(&self, order: &OrderAny, new_quantity: Quantity);
}

/// Uses [`WeakCell`] to avoid circular references between components.
#[derive(Clone, Debug)]
pub enum ModifyOrderHandlerAny {
    OrderEmulator(WeakCell<OrderEmulator>),
}

impl ModifyOrderHandler for ModifyOrderHandlerAny {
    fn handle_modify_order(&self, order: &OrderAny, new_quantity: Quantity) {
        match self {
            Self::OrderEmulator(emulator_weak) => {
                if let Some(emulator) = emulator_weak.upgrade() {
                    let mut order_clone = order.clone();
                    emulator
                        .borrow_mut()
                        .update_order(&mut order_clone, new_quantity);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_core::{UUID4, WeakCell};
    use nautilus_model::{
        enums::{OrderSide, OrderType, TriggerType},
        identifiers::{StrategyId, TraderId},
        instruments::{Instrument, stubs::audusd_sim},
        orders::{Order, OrderTestBuilder},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::order_emulator::emulator::OrderEmulator;

    fn create_test_emulator() -> Rc<RefCell<OrderEmulator>> {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));

        Rc::new(RefCell::new(OrderEmulator::new(clock, cache)))
    }

    fn create_test_stop_order(instrument: &dyn Instrument) -> OrderAny {
        OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .trigger_price(Price::from("1.00050"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build()
    }

    #[rstest]
    fn test_submit_order_handler_constructs() {
        let emulator = create_test_emulator();
        let weak_emulator = WeakCell::from(Rc::downgrade(&emulator));
        let handler = SubmitOrderHandlerAny::OrderEmulator(weak_emulator);

        assert!(matches!(handler, SubmitOrderHandlerAny::OrderEmulator(_)));
    }

    #[rstest]
    fn test_cancel_order_handler_constructs() {
        let emulator = create_test_emulator();
        let weak_emulator = WeakCell::from(Rc::downgrade(&emulator));
        let handler = CancelOrderHandlerAny::OrderEmulator(weak_emulator);

        assert!(matches!(handler, CancelOrderHandlerAny::OrderEmulator(_)));
    }

    #[rstest]
    fn test_modify_order_handler_constructs() {
        let emulator = create_test_emulator();
        let weak_emulator = WeakCell::from(Rc::downgrade(&emulator));
        let handler = ModifyOrderHandlerAny::OrderEmulator(weak_emulator);

        assert!(matches!(handler, ModifyOrderHandlerAny::OrderEmulator(_)));
    }

    #[rstest]
    fn test_cancel_order_handler_dispatch_does_not_panic() {
        let emulator = create_test_emulator();
        let weak_emulator = WeakCell::from(Rc::downgrade(&emulator));
        let handler = CancelOrderHandlerAny::OrderEmulator(weak_emulator);
        let instrument = audusd_sim();
        let order = create_test_stop_order(&instrument);

        handler.handle_cancel_order(&order);
    }

    #[rstest]
    fn test_modify_order_handler_dispatch_does_not_panic() {
        let emulator = create_test_emulator();
        let weak_emulator = WeakCell::from(Rc::downgrade(&emulator));
        let handler = ModifyOrderHandlerAny::OrderEmulator(weak_emulator);
        let instrument = audusd_sim();
        let order = create_test_stop_order(&instrument);
        let new_quantity = Quantity::from(50_000);

        handler.handle_modify_order(&order, new_quantity);
    }

    #[rstest]
    fn test_handler_with_dropped_emulator_does_not_panic() {
        let emulator = create_test_emulator();
        let weak_emulator = WeakCell::from(Rc::downgrade(&emulator));
        let handler = SubmitOrderHandlerAny::OrderEmulator(weak_emulator);
        let instrument = audusd_sim();
        let order = create_test_stop_order(&instrument);
        let command = SubmitOrder::new(
            TraderId::from("TESTER-001"),
            None,
            StrategyId::from("STRATEGY-001"),
            instrument.id(),
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::new(),
            0.into(),
        );
        drop(emulator);

        // WeakCell returns None when emulator is dropped
        handler.handle_submit_order(command);
    }
}

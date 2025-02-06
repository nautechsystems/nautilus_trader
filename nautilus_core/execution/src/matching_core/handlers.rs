use std::{cell::RefCell, rc::Rc};

use nautilus_model::orders::OrderAny;

use crate::{matching_engine::OrderMatchingEngine};
use crate::order_emulator::emulator::OrderEmulator;

pub trait FillMarketOrderHandler {
    fn fill_market_order(&self, order: &OrderAny);
}

#[derive(Clone)]
pub enum FillMarketOrderHandlerAny {
    OrderMatchingEngine(Rc<RefCell<OrderMatchingEngine>>),
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

#[derive(Clone)]
pub struct ShareableFillMarketOrderHandler(pub FillMarketOrderHandlerAny);

pub trait FillLimitOrderHandler {
    fn fill_limit_order(&self, order: &OrderAny);
}

#[derive(Clone)]
pub enum FillLimitOrderHandlerAny {
    OrderMatchingEngine(Rc<RefCell<OrderMatchingEngine>>),
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl FillLimitOrderHandler for FillLimitOrderHandlerAny {
    fn fill_limit_order(&self, order: &OrderAny) {
        match self {
            FillLimitOrderHandlerAny::OrderMatchingEngine(engine) => {
                todo!("implement fill_limit_order for OrderMatchingEngine")
            }
            FillLimitOrderHandlerAny::OrderEmulator(emulator) => {
                todo!("implement fill_limit_order for OrderEmulator")
            }
        }
    }
}

#[derive(Clone)]
pub struct ShareableFillLimitOrderHandler(pub FillLimitOrderHandlerAny);

pub trait TriggerStopOrderHandler {
    fn trigger_stop_order(&self, order: &OrderAny);
}

#[derive(Clone)]
pub enum TriggerStopOrderHandlerAny {
    OrderMatchingEngine(Rc<RefCell<OrderMatchingEngine>>),
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl TriggerStopOrderHandler for TriggerStopOrderHandlerAny {
    fn trigger_stop_order(&self, order: &OrderAny) {
        match self {
            TriggerStopOrderHandlerAny::OrderMatchingEngine(engine) => {
                todo!("implement trigger_stop_order for OrderMatchingEngine")
            }
            TriggerStopOrderHandlerAny::OrderEmulator(emulator) => {
                todo!("implement trigger_stop_order for OrderEmulator")
            }
        }
    }
}

#[derive(Clone)]
pub struct ShareableTriggerStopOrderHandler(pub TriggerStopOrderHandlerAny);

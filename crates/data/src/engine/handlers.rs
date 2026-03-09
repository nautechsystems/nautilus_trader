use std::{cell::RefCell, rc::Rc};

use nautilus_common::msgbus::Handler;
use nautilus_core::WeakCell;
use nautilus_model::data::{Bar, BarType, QuoteTick, TradeTick};
use ustr::Ustr;

use crate::aggregation::BarAggregator;

/// Message handler for processing quote ticks through bar aggregators.
///
/// This handler receives quote tick messages and forwards them to the underlying
/// bar aggregator for processing. It's used as part of the data engine's message
/// routing infrastructure to build bars from incoming quote data.
#[derive(Debug)]
pub struct BarQuoteHandler {
    aggregator: WeakCell<Box<dyn BarAggregator>>,
    bar_type: BarType,
}

impl BarQuoteHandler {
    pub(crate) fn new(aggregator: Rc<RefCell<Box<dyn BarAggregator>>>, bar_type: BarType) -> Self {
        Self {
            aggregator: WeakCell::from(Rc::downgrade(&aggregator)),
            bar_type,
        }
    }
}

impl Handler<QuoteTick> for BarQuoteHandler {
    fn id(&self) -> Ustr {
        Ustr::from(&format!("BarQuoteHandler|{}", self.bar_type))
    }

    fn handle(&self, quote: &QuoteTick) {
        if let Some(agg) = self.aggregator.upgrade() {
            agg.borrow_mut().handle_quote(*quote);
        }
    }
}

/// Message handler for processing trade ticks through bar aggregators.
///
/// This handler receives trade tick messages and forwards them to the underlying
/// bar aggregator for processing. It's used as part of the data engine's message
/// routing infrastructure to build bars from incoming trade data.
#[derive(Debug)]
pub struct BarTradeHandler {
    aggregator: WeakCell<Box<dyn BarAggregator>>,
    bar_type: BarType,
}

impl BarTradeHandler {
    pub(crate) fn new(aggregator: Rc<RefCell<Box<dyn BarAggregator>>>, bar_type: BarType) -> Self {
        Self {
            aggregator: WeakCell::from(Rc::downgrade(&aggregator)),
            bar_type,
        }
    }
}

impl Handler<TradeTick> for BarTradeHandler {
    fn id(&self) -> Ustr {
        Ustr::from(&format!("BarTradeHandler|{}", self.bar_type))
    }

    fn handle(&self, trade: &TradeTick) {
        if let Some(agg) = self.aggregator.upgrade() {
            agg.borrow_mut().handle_trade(*trade);
        }
    }
}

/// Message handler for processing bars through composite bar aggregators.
///
/// This handler receives bar messages and forwards them to the underlying
/// bar aggregator for further processing. It's used for building composite
/// bars from existing bars, such as creating higher timeframe bars from
/// lower timeframe bars.
#[derive(Debug)]
pub struct BarBarHandler {
    aggregator: WeakCell<Box<dyn BarAggregator>>,
    bar_type: BarType,
}

impl BarBarHandler {
    pub(crate) fn new(aggregator: Rc<RefCell<Box<dyn BarAggregator>>>, bar_type: BarType) -> Self {
        Self {
            aggregator: WeakCell::from(Rc::downgrade(&aggregator)),
            bar_type,
        }
    }
}

impl Handler<Bar> for BarBarHandler {
    fn id(&self) -> Ustr {
        Ustr::from(&format!("BarBarHandler|{}", self.bar_type))
    }

    fn handle(&self, bar: &Bar) {
        if let Some(agg) = self.aggregator.upgrade() {
            agg.borrow_mut().handle_bar(*bar);
        }
    }
}

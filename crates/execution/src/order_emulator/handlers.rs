use std::any::Any;

use nautilus_common::{messages::execution::TradingCommand, msgbus::Handler};
use nautilus_core::WeakCell;
use nautilus_model::events::OrderEventAny;
use ustr::Ustr;

use super::emulator::OrderEmulator;

#[derive(Debug)]
pub struct OrderEmulatorExecuteHandler {
    id: Ustr,
    emulator: WeakCell<OrderEmulator>,
}

impl OrderEmulatorExecuteHandler {
    #[inline]
    #[must_use]
    pub const fn new(id: Ustr, emulator: WeakCell<OrderEmulator>) -> Self {
        Self { id, emulator }
    }
}

impl Handler<dyn Any> for OrderEmulatorExecuteHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        if let Some(emulator) = self.emulator.upgrade() {
            if let Some(command) = msg.downcast_ref::<TradingCommand>() {
                emulator.borrow_mut().execute(command.clone());
            } else {
                log::error!("OrderEmulator received unexpected message type");
            }
        }
    }
}

#[derive(Debug)]
pub struct OrderEmulatorOnEventHandler {
    id: Ustr,
    emulator: WeakCell<OrderEmulator>,
}

impl OrderEmulatorOnEventHandler {
    #[inline]
    #[must_use]
    pub const fn new(id: Ustr, emulator: WeakCell<OrderEmulator>) -> Self {
        Self { id, emulator }
    }
}

impl Handler<OrderEventAny> for OrderEmulatorOnEventHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, event: &OrderEventAny) {
        if let Some(emulator) = self.emulator.upgrade() {
            emulator.borrow_mut().on_event(event.clone());
        }
    }
}

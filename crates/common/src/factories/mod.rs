//! Factories for constructing domain objects such as orders and events.

pub mod event;
pub mod order;

pub use event::OrderEventFactory;
pub use order::OrderFactory;

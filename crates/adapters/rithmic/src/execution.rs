//! Rithmic execution client for order management.
//!
//! This module provides the execution client that connects to Rithmic's
//! order plant for submitting, modifying, and cancelling orders.

mod client;
mod handler;
mod parse;

pub use client::{
    ExecutionEvent, OrderAccepted, OrderCancelled, OrderContext, OrderFilled, OrderModified,
    OrderRejected, OrderRequest, OrderState, OrderSubmitted, RithmicExecutionClient,
};
pub use handler::ExecutionHandler;

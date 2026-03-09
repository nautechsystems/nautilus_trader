//! Message types for system communication.
//!
//! This module provides message types used for communication between different
//! parts of the NautilusTrader system, including data requests, execution commands,
//! and system control messages.

use nautilus_model::{
    data::{Data, FundingRateUpdate, InstrumentStatus},
    events::{AccountState, OrderEventAny},
    instruments::InstrumentAny,
};
use strum::Display;

pub mod data;
pub mod execution;
pub mod system;

#[cfg(feature = "defi")]
pub mod defi;

// Re-exports
pub use data::{DataResponse, SubscribeCommand, UnsubscribeCommand};
pub use execution::ExecutionReport;

// TODO: Refine this to reduce disparity between enum sizes
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Display)]
pub enum DataEvent {
    Response(DataResponse),
    Data(Data),
    Instrument(InstrumentAny), // TODO: Eventually this can be `Data` once Cython is gone
    FundingRate(FundingRateUpdate),
    InstrumentStatus(InstrumentStatus),
    // nautilus-import-ok: conditional compilation import
    #[cfg(feature = "defi")]
    DeFi(nautilus_model::defi::data::DefiData),
}

/// Execution event variants for order events and reports.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Display)]
pub enum ExecutionEvent {
    Order(OrderEventAny),
    Report(ExecutionReport),
    Account(AccountState),
}

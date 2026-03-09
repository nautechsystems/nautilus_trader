//! Order book components which can handle L1/L2/L3 data.

pub mod aggregation;
pub mod analysis;
pub mod book;
pub mod display;
pub mod error;
pub mod ladder;
pub mod level;
pub mod own;

#[cfg(test)]
mod tests;

// Re-exports
pub use crate::orderbook::{
    book::OrderBook,
    error::{BookIntegrityError, BookViewError, InvalidBookOperation},
    ladder::BookPrice,
    level::BookLevel,
    own::OwnBookOrder,
};

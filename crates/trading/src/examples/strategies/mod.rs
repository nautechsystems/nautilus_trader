//! Example trading strategies for backtesting and demonstration.

pub mod ema_cross;
pub mod grid_mm;

pub use ema_cross::EmaCross;
pub use grid_mm::{GridMarketMaker, GridMarketMakerConfig};

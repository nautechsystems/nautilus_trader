//! Moving average type indicators.

pub mod ama;
pub mod dema;
pub mod ema;
pub mod hma;
pub mod lr;
pub mod rma;
pub mod sma;
pub mod vidya;
pub mod vwap;
pub mod wma;

use nautilus_model::enums::PriceType;
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

use crate::{
    average::{
        dema::DoubleExponentialMovingAverage, ema::ExponentialMovingAverage,
        hma::HullMovingAverage, rma::WilderMovingAverage, sma::SimpleMovingAverage,
    },
    indicator::MovingAverage,
};

#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.indicators",
        from_py_object,
    )
)]
pub enum MovingAverageType {
    Simple,
    Exponential,
    DoubleExponential,
    Wilder,
    Hull,
}

#[derive(Debug)]
pub struct MovingAverageFactory;

impl MovingAverageFactory {
    #[must_use]
    pub fn create(
        moving_average_type: MovingAverageType,
        period: usize,
    ) -> Box<dyn MovingAverage + Send + Sync> {
        let price_type = Some(PriceType::Last);

        match moving_average_type {
            MovingAverageType::Simple => Box::new(SimpleMovingAverage::new(period, price_type)),
            MovingAverageType::Exponential => {
                Box::new(ExponentialMovingAverage::new(period, price_type))
            }
            MovingAverageType::DoubleExponential => {
                Box::new(DoubleExponentialMovingAverage::new(period, price_type))
            }
            MovingAverageType::Wilder => Box::new(WilderMovingAverage::new(period, price_type)),
            MovingAverageType::Hull => Box::new(HullMovingAverage::new(period, price_type)),
        }
    }
}

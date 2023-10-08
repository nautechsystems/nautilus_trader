// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------
use nautilus_model::enums::PriceType;
use pyo3::prelude::*;
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

use crate::{
    average::{
        dema::DoubleExponentialMovingAverage, ema::ExponentialMovingAverage,
        sma::SimpleMovingAverage,
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
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum MovingAverageType {
    Simple,
    Exponential,
    DoubleExponential,
}

pub struct MovingAverageFactory;

impl MovingAverageFactory {
    pub fn create(
        moving_average_type: MovingAverageType,
        period: usize,
    ) -> Box<dyn MovingAverage + Send> {
        let price_type = Some(PriceType::Last);

        match moving_average_type {
            MovingAverageType::Simple => {
                Box::new(SimpleMovingAverage::new(period, price_type).unwrap())
            }
            MovingAverageType::Exponential => {
                Box::new(ExponentialMovingAverage::new(period, price_type).unwrap())
            }
            MovingAverageType::DoubleExponential => {
                Box::new(DoubleExponentialMovingAverage::new(period, price_type).unwrap())
            }
        }
    }
}

pub mod ama;
pub mod dema;
pub mod ema;
pub mod sma;

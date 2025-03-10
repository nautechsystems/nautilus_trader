// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::IntoPyObjectNautilusExt;
use pyo3::{basic::CompareOp, prelude::*};
use rust_decimal::Decimal;

use crate::{
    data::bet::{Bet, BetPosition, calc_bets_pnl, inverse_probability_to_bet, probability_to_bet},
    enums::{BetSide, OrderSide},
};

#[pymethods]
impl Bet {
    #[new]
    fn py_new(price: Decimal, stake: Decimal, side: BetSide) -> PyResult<Self> {
        Ok(Bet::new(price, stake, side))
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    /// Create a bet from a stake or liability, depending on the bet side.
    #[staticmethod]
    #[pyo3(name = "from_stake_or_liability")]
    fn py_from_stake_or_liability(
        price: Decimal,
        volume: Decimal,
        side: BetSide,
    ) -> PyResult<Self> {
        Ok(Bet::from_stake_or_liability(price, volume, side))
    }

    /// Create a bet from a given stake.
    #[staticmethod]
    #[pyo3(name = "from_stake")]
    fn py_from_stake(price: Decimal, stake: Decimal, side: BetSide) -> PyResult<Self> {
        Ok(Bet::from_stake(price, stake, side))
    }

    /// Create a bet from a given liability.
    ///
    /// Raises a ValueError if the bet side is not Lay.
    #[staticmethod]
    #[pyo3(name = "from_liability")]
    fn py_from_liability(price: Decimal, liability: Decimal, side: BetSide) -> PyResult<Self> {
        Ok(Bet::from_liability(price, liability, side))
    }

    /// Returns the bet's price.
    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Decimal {
        self.price()
    }

    /// Returns the bet's stake.
    #[getter]
    #[pyo3(name = "stake")]
    fn py_stake(&self) -> Decimal {
        self.stake()
    }

    /// Returns the bet's side.
    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> BetSide {
        self.side()
    }

    /// Returns the exposure of the bet.
    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> Decimal {
        self.exposure()
    }

    /// Returns the liability of the bet.
    #[pyo3(name = "liability")]
    fn py_liability(&self) -> Decimal {
        self.liability()
    }

    /// Returns the profit of the bet.
    #[pyo3(name = "profit")]
    fn py_profit(&self) -> Decimal {
        self.profit()
    }

    /// Returns the outcome win payoff.
    #[pyo3(name = "outcome_win_payoff")]
    fn py_outcome_win_payoff(&self) -> Decimal {
        self.outcome_win_payoff()
    }

    /// Returns the outcome lose payoff.
    #[pyo3(name = "outcome_lose_payoff")]
    fn py_outcome_lose_payoff(&self) -> Decimal {
        self.outcome_lose_payoff()
    }

    /// Returns the hedging stake for a given new price.
    #[pyo3(name = "hedging_stake")]
    fn py_hedging_stake(&self, price: Decimal) -> Decimal {
        self.hedging_stake(price)
    }

    /// Returns a hedging bet for a given new price.
    #[pyo3(name = "hedging_bet")]
    fn py_hedging_bet(&self, price: Decimal) -> Self {
        self.hedging_bet(price)
    }
}

#[pymethods]
impl BetPosition {
    #[new]
    fn py_new() -> Self {
        Self::default()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    /// Returns the aggregated price.
    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Decimal {
        self.price()
    }

    /// Returns the side of the position.
    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> Option<BetSide> {
        self.side()
    }

    /// Returns the aggregated exposure.
    #[getter]
    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> Decimal {
        self.exposure()
    }

    /// Returns the realized PnL.
    #[getter]
    #[pyo3(name = "realized_pnl")]
    fn py_realized_pnl(&self) -> Decimal {
        self.realized_pnl()
    }

    /// Adds a bet to the position.
    #[pyo3(name = "add_bet")]
    fn py_add_bet(&mut self, bet: &Bet) {
        self.add_bet(bet.clone());
    }

    /// Converts the position into a single Bet, if possible.
    #[pyo3(name = "as_bet")]
    fn py_as_bet(&self) -> Option<Bet> {
        self.as_bet()
    }

    /// Calculates the unrealized PnL given a current price.
    #[pyo3(name = "unrealized_pnl")]
    fn py_unrealized_pnl(&self, price: Decimal) -> Decimal {
        self.unrealized_pnl(price)
    }

    /// Calculates the total PnL (realized + unrealized) given a current price.
    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(&self, price: Decimal) -> Decimal {
        self.total_pnl(price)
    }

    /// Returns a bet that would flatten (neutralize) the position.
    #[pyo3(name = "flattening_bet")]
    fn py_flattening_bet(&self, price: Decimal) -> Option<Bet> {
        self.flattening_bet(price)
    }

    /// Resets the position.
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

#[pyfunction]
#[pyo3(name = "calc_bets_pnl")]
pub fn py_calc_bets_pnl(bets: Vec<Bet>) -> PyResult<Decimal> {
    Ok(calc_bets_pnl(&bets))
}

#[pyfunction]
#[pyo3(name = "probability_to_bet")]
pub fn py_probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSide,
) -> PyResult<Bet> {
    Ok(probability_to_bet(probability, volume, side.as_specified()))
}

#[pyfunction]
#[pyo3(name = "inverse_probability_to_bet")]
pub fn py_inverse_probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSide,
) -> PyResult<Bet> {
    Ok(inverse_probability_to_bet(
        probability,
        volume,
        side.as_specified(),
    ))
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use pyo3::{basic::CompareOp, prelude::*};
use rust_decimal::Decimal;

use crate::{
    data::bet::{Bet, BetPosition, calc_bets_pnl, inverse_probability_to_bet, probability_to_bet},
    enums::{BetSide, OrderSide},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Bet {
    /// A bet in a betting market.
    #[new]
    fn py_new(price: Decimal, stake: Decimal, side: BetSide) -> Self {
        Self::new(price, stake, side)
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

    /// Creates a bet from a stake or liability depending on the bet side.
    ///
    /// For `BetSide::Back` this calls `Self.from_stake` and for
    /// `BetSide::Lay` it calls `Self.from_liability`.
    #[staticmethod]
    #[pyo3(name = "from_stake_or_liability")]
    fn py_from_stake_or_liability(price: Decimal, volume: Decimal, side: BetSide) -> Self {
        Self::from_stake_or_liability(price, volume, side)
    }

    /// Creates a bet from a given stake.
    #[staticmethod]
    #[pyo3(name = "from_stake")]
    fn py_from_stake(price: Decimal, stake: Decimal, side: BetSide) -> Self {
        Self::from_stake(price, stake, side)
    }

    /// Creates a bet from a given liability.
    #[staticmethod]
    #[pyo3(name = "from_liability")]
    fn py_from_liability(price: Decimal, liability: Decimal, side: BetSide) -> Self {
        Self::from_liability(price, liability, side)
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

    /// Returns the bet's exposure.
    ///
    /// For BACK bets, exposure is positive; for LAY bets, it is negative.
    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> Decimal {
        self.exposure()
    }

    /// Returns the bet's liability.
    ///
    /// For BACK bets, liability equals the stake; for LAY bets, it is
    /// stake multiplied by (price - 1).
    #[pyo3(name = "liability")]
    fn py_liability(&self) -> Decimal {
        self.liability()
    }

    /// Returns the bet's profit.
    ///
    /// For BACK bets, profit is stake * (price - 1); for LAY bets it equals the stake.
    #[pyo3(name = "profit")]
    fn py_profit(&self) -> Decimal {
        self.profit()
    }

    /// Returns the outcome win payoff.
    ///
    /// For BACK bets this is the profit; for LAY bets it is the negative liability.
    #[pyo3(name = "outcome_win_payoff")]
    fn py_outcome_win_payoff(&self) -> Decimal {
        self.outcome_win_payoff()
    }

    /// Returns the outcome lose payoff.
    ///
    /// For BACK bets this is the negative liability; for LAY bets it is the profit.
    #[pyo3(name = "outcome_lose_payoff")]
    fn py_outcome_lose_payoff(&self) -> Decimal {
        self.outcome_lose_payoff()
    }

    /// Returns the hedging stake given a new price.
    #[pyo3(name = "hedging_stake")]
    fn py_hedging_stake(&self, price: Decimal) -> Decimal {
        self.hedging_stake(price)
    }

    /// Creates a hedging bet for a given price.
    #[pyo3(name = "hedging_bet")]
    fn py_hedging_bet(&self, price: Decimal) -> Self {
        self.hedging_bet(price)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BetPosition {
    /// A position comprising one or more bets.
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

    /// Returns the position's price.
    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Decimal {
        self.price()
    }

    /// Returns the overall side of the position.
    ///
    /// If exposure is positive the side is BACK; if negative, LAY; if zero, None.
    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> Option<BetSide> {
        self.side()
    }

    /// Returns the position's exposure.
    #[getter]
    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> Decimal {
        self.exposure()
    }

    /// Returns the position's realized profit and loss.
    #[getter]
    #[pyo3(name = "realized_pnl")]
    fn py_realized_pnl(&self) -> Decimal {
        self.realized_pnl()
    }

    /// Adds a bet to the position, adjusting exposure and realized PnL.
    #[pyo3(name = "add_bet")]
    fn py_add_bet(&mut self, bet: &Bet) {
        self.add_bet(bet.clone());
    }

    /// Converts the current position into a single bet, if possible.
    #[pyo3(name = "as_bet")]
    fn py_as_bet(&self) -> Option<Bet> {
        self.as_bet()
    }

    /// Calculates the unrealized profit and loss given a current price.
    #[pyo3(name = "unrealized_pnl")]
    fn py_unrealized_pnl(&self, price: Decimal) -> Decimal {
        self.unrealized_pnl(price)
    }

    /// Returns the total profit and loss (realized plus unrealized) given a current price.
    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(&self, price: Decimal) -> Decimal {
        self.total_pnl(price)
    }

    /// Creates a bet that would flatten (neutralize) the current position.
    #[pyo3(name = "flattening_bet")]
    fn py_flattening_bet(&self, price: Decimal) -> Option<Bet> {
        self.flattening_bet(price)
    }

    /// Resets the bet position to its initial state.
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

/// Calculates the combined profit and loss for a slice of bets.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "calc_bets_pnl")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_calc_bets_pnl(bets: Vec<Bet>) -> PyResult<Decimal> {
    Ok(calc_bets_pnl(&bets))
}

/// Converts a probability and volume into a Bet.
///
/// For a BUY side, this creates a BACK bet; for SELL, a LAY bet.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "probability_to_bet")]
pub fn py_probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSide,
) -> PyResult<Bet> {
    probability_to_bet(probability, volume, side.as_specified()).map_err(to_pyvalue_err)
}

/// Converts a probability and volume into a Bet using the inverse probability.
///
/// The side is also inverted (BUY becomes SELL and vice versa).
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "inverse_probability_to_bet")]
pub fn py_inverse_probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSide,
) -> PyResult<Bet> {
    inverse_probability_to_bet(probability, volume, side.as_specified()).map_err(to_pyvalue_err)
}

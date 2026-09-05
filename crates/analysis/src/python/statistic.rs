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

use std::{fmt::Debug, sync::Arc};

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::position::Position;
use pyo3::{
    exceptions::PyAttributeError,
    prelude::*,
    types::{PyDict, PyList},
};

use crate::{
    Returns,
    analyzer::Statistic,
    statistic::PortfolioStatistic,
    statistics::{
        alpha::Alpha, beta_ratio::BetaRatio, cagr::CAGR, calmar_ratio::CalmarRatio,
        down_capture_ratio::DownCaptureRatio, expectancy::Expectancy,
        expected_shortfall::ExpectedShortfall, information_ratio::InformationRatio,
        long_ratio::LongRatio, loser_avg::AvgLoser, loser_max::MaxLoser, loser_min::MinLoser,
        max_drawdown::MaxDrawdown, omega_ratio::OmegaRatio, profit_factor::ProfitFactor,
        returns_avg::ReturnsAverage, returns_avg_loss::ReturnsAverageLoss,
        returns_avg_win::ReturnsAverageWin, returns_kurtosis::ReturnsKurtosis,
        returns_skewness::ReturnsSkewness, returns_volatility::ReturnsVolatility,
        risk_return_ratio::RiskReturnRatio, sharpe_ratio::SharpeRatio, sortino_ratio::SortinoRatio,
        tail_ratio::TailRatio, tracking_error::TrackingError, treynor_ratio::TreynorRatio,
        ulcer_index::UlcerIndex, up_capture_ratio::UpCaptureRatio, value_at_risk::ValueAtRisk,
        win_rate::WinRate, winner_avg::AvgWinner, winner_max::MaxWinner, winner_min::MinWinner,
    },
};

/// A [`PortfolioStatistic`] implemented in Python.
///
/// Wraps a user-defined Python object and dispatches each input category the analyzer feeds
/// to the method of the same name. A category the object does not define, or for which it
/// returns `None`, contributes no value.
///
/// Calculated values must be numeric, matching the `f64` item type the analyzer collects.
pub struct PythonStatistic {
    name: String,
    statistic: Py<PyAny>,
}

impl Debug for PythonStatistic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PythonStatistic))
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl PythonStatistic {
    /// Creates a new [`PythonStatistic`] wrapping `statistic`.
    ///
    /// The name is resolved once at construction, so it stays stable for the registration key
    /// and every later lookup.
    ///
    /// # Errors
    ///
    /// Returns an error if `statistic` has no `name` attribute resolving to a non-empty string.
    pub fn new(py: Python<'_>, statistic: Py<PyAny>) -> PyResult<Self> {
        let name = statistic
            .getattr(py, "name")
            .and_then(|name| name.extract::<String>(py))
            .map_err(|e| {
                to_pyvalue_err(format!(
                    "Invalid statistic: `name` must resolve to a string, was {e}"
                ))
            })?;

        if name.trim().is_empty() {
            return Err(to_pyvalue_err(
                "Invalid statistic: `name` must not be empty".to_string(),
            ));
        }

        Ok(Self { name, statistic })
    }

    /// Returns the bound `method` callable, or `None` when the statistic does not define it.
    fn method<'py>(&self, py: Python<'py>, method: &str) -> Option<Bound<'py, PyAny>> {
        match self.statistic.bind(py).getattr(method) {
            Ok(callable) => Some(callable),
            Err(e) if e.is_instance_of::<PyAttributeError>(py) => None,
            Err(e) => {
                self.report(py, method, "failed attribute lookup for", e);
                None
            }
        }
    }

    /// Returns the numeric value from `result`, reporting a raised or non-numeric outcome.
    ///
    /// The trait has no error channel, so a failure is reported here and skipped rather than
    /// propagated, leaving the remaining statistics to calculate.
    fn value(
        &self,
        py: Python<'_>,
        method: &str,
        result: PyResult<Bound<'_, PyAny>>,
    ) -> Option<f64> {
        let value = match result {
            Ok(value) => value,
            Err(e) => {
                self.report(py, method, "raised in", e);
                return None;
            }
        };

        if value.is_none() {
            return None;
        }

        match value.extract::<f64>() {
            Ok(value) => Some(value),
            Err(e) => {
                self.report(py, method, "returned a non-numeric value from", e);
                None
            }
        }
    }

    /// Reports a Python-side failure through both the log and `sys.unraisablehook`.
    ///
    /// The log alone is not enough: the `log` facade is a no-op until a logger is installed,
    /// which is the usual case for a standalone analyzer, so the traceback also goes to
    /// `sys.unraisablehook` where Python surfaces uncatchable callback errors.
    fn report(&self, py: Python<'_>, method: &str, what: &str, e: PyErr) {
        log::error!("Statistic `{}` {what} `{method}`: {e}", self.name);

        e.write_unraisable(py, Some(&self.statistic.bind(py).clone()));
    }

    /// Converts `returns` into the `dict[int, float]` shape the Python analyzer surface uses.
    fn returns_dict<'py>(
        &self,
        py: Python<'py>,
        method: &str,
        returns: &Returns,
    ) -> Option<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);

        for (timestamp, value) in returns {
            self.converted(method, dict.set_item(timestamp.as_u64(), value))?;
        }

        Some(dict)
    }

    /// Returns the converted `value`, logging a conversion failure against `method`.
    fn converted<T>(&self, method: &str, value: PyResult<T>) -> Option<T> {
        match value {
            Ok(value) => Some(value),
            Err(e) => {
                log::error!(
                    "Statistic `{}` could not receive input for `{method}`: {e}",
                    self.name
                );
                None
            }
        }
    }
}

impl PortfolioStatistic for PythonStatistic {
    type Item = f64;

    fn name(&self) -> String {
        self.name.clone()
    }

    fn calculate_from_returns(&self, returns: &Returns) -> Option<f64> {
        const METHOD: &str = "calculate_from_returns";

        Python::attach(|py| {
            let method = self.method(py, METHOD)?;
            let returns = self.returns_dict(py, METHOD, returns)?;
            self.value(py, METHOD, method.call1((returns,)))
        })
    }

    fn calculate_from_realized_pnls(&self, realized_pnls: &[f64]) -> Option<f64> {
        const METHOD: &str = "calculate_from_realized_pnls";

        Python::attach(|py| {
            let method = self.method(py, METHOD)?;
            let realized_pnls = self.converted(METHOD, PyList::new(py, realized_pnls))?;
            self.value(py, METHOD, method.call1((realized_pnls,)))
        })
    }

    fn calculate_from_positions(&self, positions: &[Position]) -> Option<f64> {
        const METHOD: &str = "calculate_from_positions";

        Python::attach(|py| {
            let method = self.method(py, METHOD)?;
            let positions = self.converted(METHOD, PyList::new(py, positions.iter().cloned()))?;
            self.value(py, METHOD, method.call1((positions,)))
        })
    }

    fn calculate_from_returns_with_benchmark(
        &self,
        returns: &Returns,
        benchmark: &Returns,
    ) -> Option<f64> {
        const METHOD: &str = "calculate_from_returns_with_benchmark";

        Python::attach(|py| {
            let method = self.method(py, METHOD)?;
            let returns = self.returns_dict(py, METHOD, returns)?;
            let benchmark = self.returns_dict(py, METHOD, benchmark)?;
            self.value(py, METHOD, method.call1((returns, benchmark)))
        })
    }
}

/// Converts `statistic` into a registrable [`Statistic`].
///
/// A built-in statistic type converts to its native Rust implementation, keeping calculation in
/// Rust. Any other object is wrapped as a [`PythonStatistic`] and dispatched back into Python on
/// calculation, including a user-defined class whose name matches a built-in.
///
/// # Errors
///
/// Returns an error if the object's class cannot be resolved, or if a user-defined statistic has
/// no `name` attribute resolving to a non-empty string.
pub fn statistic_from_pyobject(py: Python<'_>, statistic: Py<PyAny>) -> PyResult<Statistic> {
    let type_name = statistic
        .getattr(py, "__class__")?
        .getattr(py, "__name__")?
        .extract::<String>(py)?;

    if let Some(statistic) = native_statistic(py, &statistic, &type_name) {
        return Ok(statistic);
    }

    Ok(Arc::new(PythonStatistic::new(py, statistic)?))
}

/// Returns the native implementation when `statistic` is an instance of the built-in `type_name`.
///
/// The name selects which built-in type to try, and extraction then confirms the instance. A
/// user-defined class that only shares a built-in name fails that check and falls through to the
/// Python bridge, so built-in names stay usable for user-defined statistics.
fn native_statistic(py: Python<'_>, statistic: &Py<PyAny>, type_name: &str) -> Option<Statistic> {
    fn extract<T>(py: Python<'_>, statistic: &Py<PyAny>) -> Option<Statistic>
    where
        T: PortfolioStatistic<Item = f64>
            + Send
            + Sync
            + 'static
            + for<'a, 'py> FromPyObject<'a, 'py>,
    {
        statistic
            .extract::<T>(py)
            .ok()
            .map(|statistic| Arc::new(statistic) as Statistic)
    }

    match type_name {
        "MaxWinner" => extract::<MaxWinner>(py, statistic),
        "MinWinner" => extract::<MinWinner>(py, statistic),
        "AvgWinner" => extract::<AvgWinner>(py, statistic),
        "MaxLoser" => extract::<MaxLoser>(py, statistic),
        "MinLoser" => extract::<MinLoser>(py, statistic),
        "AvgLoser" => extract::<AvgLoser>(py, statistic),
        "Expectancy" => extract::<Expectancy>(py, statistic),
        "WinRate" => extract::<WinRate>(py, statistic),
        "ReturnsVolatility" => extract::<ReturnsVolatility>(py, statistic),
        "ReturnsAverage" => extract::<ReturnsAverage>(py, statistic),
        "ReturnsAverageLoss" => extract::<ReturnsAverageLoss>(py, statistic),
        "ReturnsAverageWin" => extract::<ReturnsAverageWin>(py, statistic),
        "SharpeRatio" => extract::<SharpeRatio>(py, statistic),
        "SortinoRatio" => extract::<SortinoRatio>(py, statistic),
        "ProfitFactor" => extract::<ProfitFactor>(py, statistic),
        "RiskReturnRatio" => extract::<RiskReturnRatio>(py, statistic),
        "LongRatio" => extract::<LongRatio>(py, statistic),
        "CAGR" => extract::<CAGR>(py, statistic),
        "CalmarRatio" => extract::<CalmarRatio>(py, statistic),
        "MaxDrawdown" => extract::<MaxDrawdown>(py, statistic),
        "Alpha" => extract::<Alpha>(py, statistic),
        "BetaRatio" => extract::<BetaRatio>(py, statistic),
        "DownCaptureRatio" => extract::<DownCaptureRatio>(py, statistic),
        "InformationRatio" => extract::<InformationRatio>(py, statistic),
        "TrackingError" => extract::<TrackingError>(py, statistic),
        "TreynorRatio" => extract::<TreynorRatio>(py, statistic),
        "ReturnsSkewness" => extract::<ReturnsSkewness>(py, statistic),
        "ReturnsKurtosis" => extract::<ReturnsKurtosis>(py, statistic),
        "TailRatio" => extract::<TailRatio>(py, statistic),
        "UlcerIndex" => extract::<UlcerIndex>(py, statistic),
        "OmegaRatio" => extract::<OmegaRatio>(py, statistic),
        "ValueAtRisk" => extract::<ValueAtRisk>(py, statistic),
        "ExpectedShortfall" => extract::<ExpectedShortfall>(py, statistic),
        "UpCaptureRatio" => extract::<UpCaptureRatio>(py, statistic),
        _ => None,
    }
}

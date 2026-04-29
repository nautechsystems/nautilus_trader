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

//! Python bindings for execution engine and order emulator configuration.

use nautilus_model::identifiers::ClientId;
use pyo3::pymethods;

use crate::{engine::config::ExecutionEngineConfig, order_emulator::config::OrderEmulatorConfig};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ExecutionEngineConfig {
    /// Configuration for `ExecutionEngine` instances.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (
        load_cache = None,
        manage_own_order_books = None,
        snapshot_orders = None,
        snapshot_positions = None,
        snapshot_positions_interval_secs = None,
        allow_overfills = None,
        external_clients = None,
        purge_closed_orders_interval_mins = None,
        purge_closed_orders_buffer_mins = None,
        purge_closed_positions_interval_mins = None,
        purge_closed_positions_buffer_mins = None,
        purge_account_events_interval_mins = None,
        purge_account_events_lookback_mins = None,
        purge_from_database = None,
        debug = None,
    ))]
    fn py_new(
        load_cache: Option<bool>,
        manage_own_order_books: Option<bool>,
        snapshot_orders: Option<bool>,
        snapshot_positions: Option<bool>,
        snapshot_positions_interval_secs: Option<f64>,
        allow_overfills: Option<bool>,
        external_clients: Option<Vec<ClientId>>,
        purge_closed_orders_interval_mins: Option<u32>,
        purge_closed_orders_buffer_mins: Option<u32>,
        purge_closed_positions_interval_mins: Option<u32>,
        purge_closed_positions_buffer_mins: Option<u32>,
        purge_account_events_interval_mins: Option<u32>,
        purge_account_events_lookback_mins: Option<u32>,
        purge_from_database: Option<bool>,
        debug: Option<bool>,
    ) -> Self {
        Self::builder()
            .maybe_load_cache(load_cache)
            .maybe_manage_own_order_books(manage_own_order_books)
            .maybe_snapshot_orders(snapshot_orders)
            .maybe_snapshot_positions(snapshot_positions)
            .maybe_snapshot_positions_interval_secs(snapshot_positions_interval_secs)
            .maybe_allow_overfills(allow_overfills)
            .maybe_external_clients(external_clients)
            .maybe_purge_closed_orders_interval_mins(purge_closed_orders_interval_mins)
            .maybe_purge_closed_orders_buffer_mins(purge_closed_orders_buffer_mins)
            .maybe_purge_closed_positions_interval_mins(purge_closed_positions_interval_mins)
            .maybe_purge_closed_positions_buffer_mins(purge_closed_positions_buffer_mins)
            .maybe_purge_account_events_interval_mins(purge_account_events_interval_mins)
            .maybe_purge_account_events_lookback_mins(purge_account_events_lookback_mins)
            .maybe_purge_from_database(purge_from_database)
            .maybe_debug(debug)
            .build()
    }

    #[getter]
    #[pyo3(name = "load_cache")]
    const fn py_load_cache(&self) -> bool {
        self.load_cache
    }

    #[getter]
    #[pyo3(name = "manage_own_order_books")]
    const fn py_manage_own_order_books(&self) -> bool {
        self.manage_own_order_books
    }

    #[getter]
    #[pyo3(name = "snapshot_orders")]
    const fn py_snapshot_orders(&self) -> bool {
        self.snapshot_orders
    }

    #[getter]
    #[pyo3(name = "snapshot_positions")]
    const fn py_snapshot_positions(&self) -> bool {
        self.snapshot_positions
    }

    #[getter]
    #[pyo3(name = "snapshot_positions_interval_secs")]
    const fn py_snapshot_positions_interval_secs(&self) -> Option<f64> {
        self.snapshot_positions_interval_secs
    }

    #[getter]
    #[pyo3(name = "allow_overfills")]
    const fn py_allow_overfills(&self) -> bool {
        self.allow_overfills
    }

    #[getter]
    #[pyo3(name = "purge_from_database")]
    const fn py_purge_from_database(&self) -> bool {
        self.purge_from_database
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OrderEmulatorConfig {
    /// Configuration for `OrderEmulator` instances.
    #[new]
    #[pyo3(signature = (debug = None))]
    fn py_new(debug: Option<bool>) -> Self {
        Self::builder().maybe_debug(debug).build()
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

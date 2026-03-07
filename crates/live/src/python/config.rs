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

use std::{collections::HashMap, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::{UUID4, python::to_pyvalue_err};
use nautilus_model::identifiers::TraderId;
use nautilus_portfolio::config::PortfolioConfig;
use pyo3::{PyResult, pymethods};

use crate::config::{
    InstrumentProviderConfig, LiveDataClientConfig, LiveDataEngineConfig, LiveExecClientConfig,
    LiveExecEngineConfig, LiveNodeConfig, LiveRiskEngineConfig, RoutingConfig,
};

#[pymethods]
impl LiveDataEngineConfig {
    #[new]
    #[pyo3(signature = (qsize=None))]
    fn py_new(qsize: Option<u32>) -> Self {
        let default = Self::default();
        Self {
            qsize: qsize.unwrap_or(default.qsize),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn qsize(&self) -> u32 {
        self.qsize
    }
}

#[pymethods]
impl LiveRiskEngineConfig {
    #[new]
    #[pyo3(signature = (qsize=None))]
    fn py_new(qsize: Option<u32>) -> Self {
        let default = Self::default();
        Self {
            qsize: qsize.unwrap_or(default.qsize),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn qsize(&self) -> u32 {
        self.qsize
    }
}

#[pymethods]
impl LiveExecEngineConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (reconciliation=None, reconciliation_startup_delay_secs=None, reconciliation_lookback_mins=None, reconciliation_instrument_ids=None, filter_unclaimed_external_orders=None, filter_position_reports=None, filtered_client_order_ids=None, generate_missing_orders=None, inflight_check_interval_ms=None, inflight_check_threshold_ms=None, inflight_check_retries=None, open_check_interval_secs=None, open_check_lookback_mins=None, open_check_threshold_ms=None, open_check_missing_retries=None, open_check_open_only=None, max_single_order_queries_per_cycle=None, single_order_query_delay_ms=None, position_check_interval_secs=None, position_check_lookback_mins=None, position_check_threshold_ms=None, position_check_retries=None, purge_closed_orders_interval_mins=None, purge_closed_orders_buffer_mins=None, purge_closed_positions_interval_mins=None, purge_closed_positions_buffer_mins=None, purge_account_events_interval_mins=None, purge_account_events_lookback_mins=None, purge_from_database=None, own_books_audit_interval_secs=None, graceful_shutdown_on_error=None, qsize=None))]
    fn py_new(
        reconciliation: Option<bool>,
        reconciliation_startup_delay_secs: Option<f64>,
        reconciliation_lookback_mins: Option<u32>,
        reconciliation_instrument_ids: Option<Vec<String>>,
        filter_unclaimed_external_orders: Option<bool>,
        filter_position_reports: Option<bool>,
        filtered_client_order_ids: Option<Vec<String>>,
        generate_missing_orders: Option<bool>,
        inflight_check_interval_ms: Option<u32>,
        inflight_check_threshold_ms: Option<u32>,
        inflight_check_retries: Option<u32>,
        open_check_interval_secs: Option<f64>,
        open_check_lookback_mins: Option<u32>,
        open_check_threshold_ms: Option<u32>,
        open_check_missing_retries: Option<u32>,
        open_check_open_only: Option<bool>,
        max_single_order_queries_per_cycle: Option<u32>,
        single_order_query_delay_ms: Option<u32>,
        position_check_interval_secs: Option<f64>,
        position_check_lookback_mins: Option<u32>,
        position_check_threshold_ms: Option<u32>,
        position_check_retries: Option<u32>,
        purge_closed_orders_interval_mins: Option<u32>,
        purge_closed_orders_buffer_mins: Option<u32>,
        purge_closed_positions_interval_mins: Option<u32>,
        purge_closed_positions_buffer_mins: Option<u32>,
        purge_account_events_interval_mins: Option<u32>,
        purge_account_events_lookback_mins: Option<u32>,
        purge_from_database: Option<bool>,
        own_books_audit_interval_secs: Option<f64>,
        graceful_shutdown_on_error: Option<bool>,
        qsize: Option<u32>,
    ) -> Self {
        let default = Self::default();
        Self {
            reconciliation: reconciliation.unwrap_or(default.reconciliation),
            reconciliation_startup_delay_secs: reconciliation_startup_delay_secs
                .unwrap_or(default.reconciliation_startup_delay_secs),
            reconciliation_lookback_mins,
            reconciliation_instrument_ids,
            filter_unclaimed_external_orders: filter_unclaimed_external_orders
                .unwrap_or(default.filter_unclaimed_external_orders),
            filter_position_reports: filter_position_reports
                .unwrap_or(default.filter_position_reports),
            filtered_client_order_ids,
            generate_missing_orders: generate_missing_orders
                .unwrap_or(default.generate_missing_orders),
            inflight_check_interval_ms: inflight_check_interval_ms
                .unwrap_or(default.inflight_check_interval_ms),
            inflight_check_threshold_ms: inflight_check_threshold_ms
                .unwrap_or(default.inflight_check_threshold_ms),
            inflight_check_retries: inflight_check_retries
                .unwrap_or(default.inflight_check_retries),
            open_check_interval_secs,
            open_check_lookback_mins: open_check_lookback_mins.or(default.open_check_lookback_mins),
            open_check_threshold_ms: open_check_threshold_ms
                .unwrap_or(default.open_check_threshold_ms),
            open_check_missing_retries: open_check_missing_retries
                .unwrap_or(default.open_check_missing_retries),
            open_check_open_only: open_check_open_only.unwrap_or(default.open_check_open_only),
            max_single_order_queries_per_cycle: max_single_order_queries_per_cycle
                .unwrap_or(default.max_single_order_queries_per_cycle),
            single_order_query_delay_ms: single_order_query_delay_ms
                .unwrap_or(default.single_order_query_delay_ms),
            position_check_interval_secs,
            position_check_lookback_mins: position_check_lookback_mins
                .unwrap_or(default.position_check_lookback_mins),
            position_check_threshold_ms: position_check_threshold_ms
                .unwrap_or(default.position_check_threshold_ms),
            position_check_retries: position_check_retries
                .unwrap_or(default.position_check_retries),
            purge_closed_orders_interval_mins,
            purge_closed_orders_buffer_mins,
            purge_closed_positions_interval_mins,
            purge_closed_positions_buffer_mins,
            purge_account_events_interval_mins,
            purge_account_events_lookback_mins,
            purge_from_database: purge_from_database.unwrap_or(default.purge_from_database),
            own_books_audit_interval_secs,
            graceful_shutdown_on_error: graceful_shutdown_on_error
                .unwrap_or(default.graceful_shutdown_on_error),
            qsize: qsize.unwrap_or(default.qsize),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl RoutingConfig {
    #[new]
    #[pyo3(signature = (default=None, venues=None))]
    fn py_new(default: Option<bool>, venues: Option<Vec<String>>) -> Self {
        Self {
            default: default.unwrap_or(false),
            venues,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn default(&self) -> bool {
        self.default
    }

    #[getter]
    fn venues(&self) -> Option<Vec<String>> {
        self.venues.clone()
    }
}

#[pymethods]
impl InstrumentProviderConfig {
    #[new]
    #[pyo3(signature = (load_all=None, load_ids=None, filters=None))]
    fn py_new(
        load_all: Option<bool>,
        load_ids: Option<bool>,
        filters: Option<HashMap<String, String>>,
    ) -> Self {
        let default = Self::default();
        Self {
            load_all: load_all.unwrap_or(default.load_all),
            load_ids: load_ids.unwrap_or(default.load_ids),
            filters: filters.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn load_all(&self) -> bool {
        self.load_all
    }

    #[getter]
    fn load_ids(&self) -> bool {
        self.load_ids
    }

    #[getter]
    fn filters(&self) -> HashMap<String, String> {
        self.filters.clone()
    }
}

#[pymethods]
impl LiveDataClientConfig {
    #[new]
    #[pyo3(signature = (handle_revised_bars=None, instrument_provider=None, routing=None))]
    fn py_new(
        handle_revised_bars: Option<bool>,
        instrument_provider: Option<InstrumentProviderConfig>,
        routing: Option<RoutingConfig>,
    ) -> Self {
        Self {
            handle_revised_bars: handle_revised_bars.unwrap_or(false),
            instrument_provider: instrument_provider.unwrap_or_default(),
            routing: routing.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn handle_revised_bars(&self) -> bool {
        self.handle_revised_bars
    }

    #[getter]
    fn instrument_provider(&self) -> InstrumentProviderConfig {
        self.instrument_provider.clone()
    }

    #[getter]
    fn routing(&self) -> RoutingConfig {
        self.routing.clone()
    }
}

#[pymethods]
impl LiveExecClientConfig {
    #[new]
    #[pyo3(signature = (instrument_provider=None, routing=None))]
    fn py_new(
        instrument_provider: Option<InstrumentProviderConfig>,
        routing: Option<RoutingConfig>,
    ) -> Self {
        Self {
            instrument_provider: instrument_provider.unwrap_or_default(),
            routing: routing.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn instrument_provider(&self) -> InstrumentProviderConfig {
        self.instrument_provider.clone()
    }

    #[getter]
    fn routing(&self) -> RoutingConfig {
        self.routing.clone()
    }
}

#[pymethods]
impl LiveNodeConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (environment=None, trader_id=None, load_state=None, save_state=None, logging=None, instance_id=None, timeout_connection_secs=None, timeout_reconciliation_secs=None, timeout_portfolio_secs=None, timeout_disconnection_secs=None, delay_post_stop_secs=None, timeout_shutdown_secs=None, cache=None, msgbus=None, portfolio=None, data_engine=None, risk_engine=None, exec_engine=None))]
    fn py_new(
        environment: Option<Environment>,
        trader_id: Option<TraderId>,
        load_state: Option<bool>,
        save_state: Option<bool>,
        logging: Option<LoggerConfig>,
        instance_id: Option<UUID4>,
        timeout_connection_secs: Option<f64>,
        timeout_reconciliation_secs: Option<f64>,
        timeout_portfolio_secs: Option<f64>,
        timeout_disconnection_secs: Option<f64>,
        delay_post_stop_secs: Option<f64>,
        timeout_shutdown_secs: Option<f64>,
        cache: Option<CacheConfig>,
        msgbus: Option<MessageBusConfig>,
        portfolio: Option<PortfolioConfig>,
        data_engine: Option<LiveDataEngineConfig>,
        risk_engine: Option<LiveRiskEngineConfig>,
        exec_engine: Option<LiveExecEngineConfig>,
    ) -> PyResult<Self> {
        let default = Self::default();

        let to_duration = |value: f64, name: &str| -> PyResult<Duration> {
            if !value.is_finite() || !(0.0..=86_400.0).contains(&value) {
                return Err(to_pyvalue_err(format!(
                    "invalid {name}: {value} (must be finite, non-negative, and <= 86400)"
                )));
            }
            Ok(Duration::from_secs_f64(value))
        };

        Ok(Self {
            environment: environment.unwrap_or(default.environment),
            trader_id: trader_id.unwrap_or(default.trader_id),
            load_state: load_state.unwrap_or(default.load_state),
            save_state: save_state.unwrap_or(default.save_state),
            logging: logging.unwrap_or(default.logging),
            instance_id,
            timeout_connection: to_duration(
                timeout_connection_secs.unwrap_or(default.timeout_connection.as_secs_f64()),
                "timeout_connection_secs",
            )?,
            timeout_reconciliation: to_duration(
                timeout_reconciliation_secs.unwrap_or(default.timeout_reconciliation.as_secs_f64()),
                "timeout_reconciliation_secs",
            )?,
            timeout_portfolio: to_duration(
                timeout_portfolio_secs.unwrap_or(default.timeout_portfolio.as_secs_f64()),
                "timeout_portfolio_secs",
            )?,
            timeout_disconnection: to_duration(
                timeout_disconnection_secs.unwrap_or(default.timeout_disconnection.as_secs_f64()),
                "timeout_disconnection_secs",
            )?,
            delay_post_stop: to_duration(
                delay_post_stop_secs.unwrap_or(default.delay_post_stop.as_secs_f64()),
                "delay_post_stop_secs",
            )?,
            timeout_shutdown: to_duration(
                timeout_shutdown_secs.unwrap_or(default.timeout_shutdown.as_secs_f64()),
                "timeout_shutdown_secs",
            )?,
            cache,
            msgbus,
            portfolio,
            streaming: None,
            data_engine: data_engine.unwrap_or_default(),
            risk_engine: risk_engine.unwrap_or_default(),
            exec_engine: exec_engine.unwrap_or_default(),
            data_clients: HashMap::new(),
            exec_clients: HashMap::new(),
        })
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn environment(&self) -> Environment {
        self.environment
    }

    #[getter]
    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    fn load_state(&self) -> bool {
        self.load_state
    }

    #[getter]
    fn save_state(&self) -> bool {
        self.save_state
    }

    #[getter]
    fn timeout_connection_secs(&self) -> f64 {
        self.timeout_connection.as_secs_f64()
    }

    #[getter]
    fn timeout_reconciliation_secs(&self) -> f64 {
        self.timeout_reconciliation.as_secs_f64()
    }

    #[getter]
    fn timeout_portfolio_secs(&self) -> f64 {
        self.timeout_portfolio.as_secs_f64()
    }

    #[getter]
    fn timeout_disconnection_secs(&self) -> f64 {
        self.timeout_disconnection.as_secs_f64()
    }

    #[getter]
    fn delay_post_stop_secs(&self) -> f64 {
        self.delay_post_stop.as_secs_f64()
    }

    #[getter]
    fn timeout_shutdown_secs(&self) -> f64 {
        self.timeout_shutdown.as_secs_f64()
    }
}

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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
    clock::Clock,
};
use nautilus_core::UUID4;
use nautilus_data::engine::DataEngine;
use nautilus_execution::engine::ExecutionEngine;
use nautilus_model::identifiers::TraderId;
use ustr::Ustr;

use crate::config::NautilusKernelConfig;

/// Provides the core Nautilus system kernel.
pub struct NautilusKernel {
    pub name: Ustr,
    pub instance_id: UUID4,
    pub config: NautilusKernelConfig,
    pub data_engine: DataEngine,
    pub exec_engine: ExecutionEngine,
    pub cache: Rc<RefCell<Cache>>,
    pub clock: Rc<RefCell<dyn Clock>>,
}

impl NautilusKernel {
    #[must_use]
    pub fn new(name: Ustr, config: NautilusKernelConfig) -> Self {
        let instance_id = config.instance_id.unwrap_or_default();
        let data_engine = Self::initialize_data_engine();
        let exec_engine = Self::initialize_exec_engine();
        let cache = Self::initialize_cache(config.trader_id, &instance_id, config.cache.clone());
        let clock = Self::initialize_clock();
        Self {
            name,
            instance_id,
            data_engine,
            exec_engine,
            config,
            cache,
            clock,
        }
    }

    fn initialize_clock() -> Rc<RefCell<dyn Clock>> {
        todo!("initialize_clock")
    }

    fn initialize_cache(
        trader_id: TraderId,
        instance_id: &UUID4,
        cache_config: Option<CacheConfig>,
    ) -> Rc<RefCell<Cache>> {
        let cache_config = cache_config.unwrap_or_default();
        let cache_database: Option<Box<dyn CacheDatabaseAdapter>> =
            if let Some(cache_database_config) = &cache_config.database {
                todo!("initialize_cache_database")
            } else {
                None
            };

        let cache = Cache::new(Some(cache_config), cache_database);
        Rc::new(RefCell::new(cache))
    }

    fn initialize_data_engine() -> DataEngine {
        todo!("initialize_data_engine")
    }

    fn initialize_exec_engine() -> ExecutionEngine {
        todo!("initialize_exec_engine")
    }

    fn start(&self) {
        todo!("implement start")
    }

    fn stop(&self) {
        todo!("implement stop")
    }

    fn dispose(&self) {
        todo!("implement dispose")
    }

    fn cancel_all_tasks(&self) {
        todo!("implement cancel_all_tasks")
    }

    fn start_engines(&self) {
        todo!("implement start_engines")
    }

    fn register_executor(&self) {
        todo!("implement register_executor")
    }

    fn stop_engines(&self) {
        todo!("implement stop_engines")
    }

    fn connect_clients(&self) {
        todo!("implement connect_clients")
    }

    fn disconnect_clients(&self) {
        todo!("implement disconnect_clients")
    }

    fn stop_clients(&self) {
        todo!("implement stop_clients")
    }

    fn initialize_portfolio(&self) {
        todo!("implement initialize_portfolio")
    }

    fn await_engines_connected(&self) {
        todo!("implement await_engines_connected")
    }

    fn await_execution_reconciliation(&self) {
        todo!("implement await_execution_reconciliation")
    }

    fn await_portfolio_initialized(&self) {
        todo!("implement await_portfolio_initialized")
    }

    fn await_trader_residuals(&self) {
        todo!("implement await_trader_residuals")
    }

    fn check_engines_connected(&self) {
        todo!("implement check_engines_connected")
    }

    fn check_engines_disconnected(&self) {
        todo!("implement check_engines_disconnected")
    }

    fn check_portfolio_initialized(&self) {
        todo!("implement check_portfolio_initialized")
    }

    fn cancel_timers(&self) {
        todo!("implement cancel_timers")
    }

    fn flush_writer(&self) {
        todo!("implement flush_writer")
    }
}

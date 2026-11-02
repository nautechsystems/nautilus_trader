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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clients::ExecutionClient, clock::VirtualClock};
use nautilus_execution::engine::ExecutionEngine;
use nautilus_live::manager::{ExecutionManager, ExecutionManagerConfig};
use nautilus_model::identifiers::TraderId;

mod futures;
mod http_contracts;
mod http_retry;
mod python;
mod spot;

fn execution_reconciliation(
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    client: Box<dyn ExecutionClient>,
) -> (ExecutionManager, Rc<RefCell<ExecutionEngine>>) {
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let config = ExecutionManagerConfig {
        trader_id,
        ..Default::default()
    };

    let manager = ExecutionManager::new(clock.clone(), cache.clone(), config).unwrap();
    let mut engine = ExecutionEngine::new(clock, cache, None);
    engine.register_client(client).unwrap();
    (manager, Rc::new(RefCell::new(engine)))
}

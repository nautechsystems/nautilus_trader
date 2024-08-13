// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_common::logging::logger::Logger;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::data::Data;

use crate::exchange::SimulatedExchange;

#[warn(dead_code)]
pub trait SimulationModule {
    fn register_venue(&self, exchange: SimulatedExchange);
    fn pre_process(&self, data: Data);
    fn process(&self, ts_now: UnixNanos);
    fn log_diagnostics(&self, logger: Logger);
    fn reset(&self);
}

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

use nautilus_model::identifiers::strategy_id::StrategyId;

pub mod client_order_id;
pub mod order_list_id;
pub mod position_id_generator;

pub trait IdentifierGenerator<T> {
    fn set_count(&mut self, count: usize, strategy_id: Option<StrategyId>);

    fn reset(&mut self);

    fn count(&self, strategy_id: Option<StrategyId>) -> usize;

    fn generate(&mut self, strategy_id: Option<StrategyId>, flipped: Option<bool>) -> T;

    fn get_datetime_tag(&mut self) -> String;
}

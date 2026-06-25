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

//! Order management component.

use nautilus_common::messages::execution::SubmitOrder;
use nautilus_model::{
    events::OrderEventAny, identifiers::ExecAlgorithmId, orders::OrderAny, types::Quantity,
};

pub mod manager;

/// Describes work decided by [`manager::OrderManager`] for its owner.
#[derive(Debug, Clone)]
pub enum OrderManagerAction {
    PublishInitialized(OrderEventAny),
    SubmitToEmulator(SubmitOrder),
    SubmitToRisk(SubmitOrder),
    SubmitToAlgorithm {
        command: SubmitOrder,
        exec_algorithm_id: ExecAlgorithmId,
    },
    CancelLocal(OrderAny),
    ModifyLocalQuantity {
        order: OrderAny,
        quantity: Quantity,
    },
}

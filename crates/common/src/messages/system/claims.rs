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

use nautilus_model::identifiers::{InstrumentId, StrategyId};

/// Command requesting registration of external order claims on a running live node.
#[derive(Debug)]
pub struct RegisterExternalOrderClaims {
    pub strategy_id: StrategyId,
    pub claims: Vec<InstrumentId>,
    pub response: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
}

/// Command requesting deregistration of external order claims on a running live node.
#[derive(Debug)]
pub struct DeregisterExternalOrderClaims {
    pub strategy_id: StrategyId,
    pub response: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
}

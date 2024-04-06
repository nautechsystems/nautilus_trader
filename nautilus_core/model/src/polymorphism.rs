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

use nautilus_core::time::UnixNanos;

use crate::{
    enums::{OrderSide, OrderSideSpecified, TriggerType},
    identifiers::{
        client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, strategy_id::StrategyId, venue_order_id::VenueOrderId,
    },
    types::price::Price,
};

pub trait GetTsInit {
    fn ts_init(&self) -> UnixNanos;
}

pub trait GetInstrumentId {
    fn instrument_id(&self) -> InstrumentId;
}

pub trait GetClientOrderId {
    fn client_order_id(&self) -> ClientOrderId;
}

pub trait GetVenueOrderId {
    fn venue_order_id(&self) -> Option<VenueOrderId>;
}

pub trait GetStrategyId {
    fn strategy_id(&self) -> StrategyId;
}

pub trait GetExecAlgorithmId {
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId>;
}

pub trait GetExecSpawnId {
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
}

pub trait GetOrderSide {
    fn order_side(&self) -> OrderSide;
}

pub trait GetOrderSideSpecified {
    fn order_side_specified(&self) -> OrderSideSpecified;
}

pub trait GetEmulationTrigger {
    fn emulation_trigger(&self) -> Option<TriggerType>;
}

pub trait GetLimitPrice {
    fn limit_px(&self) -> Price;
}

pub trait GetStopPrice {
    fn stop_px(&self) -> Price;
}

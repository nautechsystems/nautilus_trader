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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod client;
pub mod config;
pub mod node;
pub mod runtime;

use nautilus_common::messages::{
    data::{
        SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeCustomData,
        SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices,
        SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeCustomData,
        UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
        UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
        UnsubscribeMarkPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
    },
    execution::{
        BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_portfolio::config::PortfolioConfig;
use pyo3::prelude::*;

pyo3_stub_gen::reexport_module_members!(
    "nautilus_trader.live",
    "nautilus_trader.portfolio",
    "PortfolioConfig"
);

/// Exposed through `nautilus_trader.live`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn live(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<runtime::ClientRuntime>()?;
    m.add_class::<SubscribeCustomData>()?;
    m.add_class::<SubscribeInstruments>()?;
    m.add_class::<SubscribeInstrument>()?;
    m.add_class::<SubscribeBookDeltas>()?;
    m.add_class::<SubscribeBookDepth10>()?;
    m.add_class::<SubscribeQuotes>()?;
    m.add_class::<SubscribeTrades>()?;
    m.add_class::<SubscribeMarkPrices>()?;
    m.add_class::<SubscribeIndexPrices>()?;
    m.add_class::<SubscribeFundingRates>()?;
    m.add_class::<SubscribeBars>()?;
    m.add_class::<SubscribeInstrumentStatus>()?;
    m.add_class::<SubscribeInstrumentClose>()?;
    m.add_class::<SubscribeOptionGreeks>()?;
    m.add_class::<UnsubscribeCustomData>()?;
    m.add_class::<UnsubscribeInstruments>()?;
    m.add_class::<UnsubscribeInstrument>()?;
    m.add_class::<UnsubscribeBookDeltas>()?;
    m.add_class::<UnsubscribeBookDepth10>()?;
    m.add_class::<UnsubscribeQuotes>()?;
    m.add_class::<UnsubscribeTrades>()?;
    m.add_class::<UnsubscribeMarkPrices>()?;
    m.add_class::<UnsubscribeIndexPrices>()?;
    m.add_class::<UnsubscribeFundingRates>()?;
    m.add_class::<UnsubscribeBars>()?;
    m.add_class::<UnsubscribeInstrumentStatus>()?;
    m.add_class::<UnsubscribeInstrumentClose>()?;
    m.add_class::<UnsubscribeOptionGreeks>()?;
    m.add_class::<SubmitOrder>()?;
    m.add_class::<SubmitOrderList>()?;
    m.add_class::<ModifyOrder>()?;
    m.add_class::<BatchModifyOrders>()?;
    m.add_class::<CancelOrder>()?;
    m.add_class::<CancelAllOrders>()?;
    m.add_class::<BatchCancelOrders>()?;
    m.add_class::<QueryAccount>()?;
    m.add_class::<QueryOrder>()?;
    m.add_class::<GenerateOrderStatusReport>()?;
    m.add_class::<GenerateOrderStatusReports>()?;
    m.add_class::<GenerateFillReports>()?;
    m.add_class::<GeneratePositionStatusReports>()?;
    client::requests::register(m)?;
    client::responses::register(m)?;
    m.add_class::<client::PyClientCache>()?;
    m.add_class::<client::ClientOutput>()?;
    m.add_class::<node::PyLiveNode>()?;
    m.add_class::<node::PyLiveNodeHandle>()?;
    m.add_class::<node::PyLiveNodeBuilder>()?;
    m.add_class::<node::NodeState>()?;
    m.add_class::<crate::config::LiveNodeConfig>()?;
    m.add_class::<crate::config::LiveDataEngineConfig>()?;
    m.add_class::<crate::config::LiveRiskEngineConfig>()?;
    m.add_class::<crate::config::LiveExecutionEngineConfig>()?;
    m.add_class::<crate::config::PluginConfig>()?;
    m.add_class::<crate::config::QueueMonitorConfig>()?;
    m.add_class::<crate::config::RoutingConfig>()?;
    m.add_class::<crate::config::InstrumentProviderConfig>()?;
    m.add_class::<crate::config::DataClientConfig>()?;
    m.add_class::<crate::config::ExecutionClientConfig>()?;
    m.add_class::<PortfolioConfig>()?;
    Ok(())
}

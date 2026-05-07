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

/// Event kind for the rust-ibapi place-order response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbPlaceOrderEvent {
    OrderStatus,
    OpenOrder,
    ExecutionData,
    CommissionReport,
    Message,
}

/// Event kind for the rust-ibapi order-update enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrderUpdateEvent {
    OrderStatus,
    OpenOrder,
    ExecutionData,
    CommissionReport,
    Message,
}

/// Event kind for the rust-ibapi cancel-order response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbCancelOrderEvent {
    OrderStatus,
    Notice,
}

/// Event kind for the rust-ibapi order query response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrdersEvent {
    OrderData,
    OrderStatus,
    Notice,
}

/// Event kind for the rust-ibapi executions response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbExecutionsEvent {
    ExecutionData,
    CommissionReport,
    Notice,
}

/// Event kind for the rust-ibapi exercise-options response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbExerciseOptionsEvent {
    OpenOrder,
    OrderStatus,
    Notice,
}

/// Event kind for rust-ibapi historical bar update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalBarUpdateEvent {
    Historical,
    Update,
    End,
}

/// Event kind for rust-ibapi realtime market-depth streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbMarketDepthEvent {
    MarketDepth,
    MarketDepthL2,
    Notice,
}

/// Event kind for rust-ibapi realtime tick streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTickEvent {
    Price,
    Size,
    String,
    Efp,
    Generic,
    OptionComputation,
    SnapshotEnd,
    Notice,
    RequestParameters,
    PriceSize,
}

/// Event kind for rust-ibapi account summary streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAccountSummaryEvent {
    Summary,
    End,
}

/// Event kind for rust-ibapi position update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbPositionUpdateEvent {
    Position,
    PositionEnd,
}

/// Event kind for rust-ibapi model-code scoped position update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbPositionUpdateMultiEvent {
    Position,
    PositionEnd,
}

/// Event kind for rust-ibapi account update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAccountUpdateEvent {
    AccountValue,
    PortfolioValue,
    UpdateTime,
    End,
}

/// Event kind for rust-ibapi model-code scoped account update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAccountUpdateMultiEvent {
    AccountMultiValue,
    End,
}

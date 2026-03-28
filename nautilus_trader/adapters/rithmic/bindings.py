# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""Public Python re-exports for the compiled Rithmic bindings."""

from nautilus_trader.core import nautilus_pyo3


_rithmic = nautilus_pyo3.rithmic

ConnectionState = _rithmic.ConnectionState
ExecutionEvent = _rithmic.ExecutionEvent
MarketDataEvent = _rithmic.MarketDataEvent
OrderAccepted = _rithmic.OrderAccepted
OrderCancelled = _rithmic.OrderCancelled
OrderFilled = _rithmic.OrderFilled
OrderModified = _rithmic.OrderModified
OrderRejected = _rithmic.OrderRejected
OrderSide = _rithmic.OrderSide
OrderStatus = _rithmic.OrderStatus
OrderSubmitted = _rithmic.OrderSubmitted
OrderType = _rithmic.OrderType
QuoteTick = _rithmic.QuoteTick
RithmicDataClient = _rithmic.RithmicDataClient
RithmicExecutionClient = _rithmic.RithmicExecutionClient
RithmicGateway = _rithmic.RithmicGateway
RithmicInstrument = _rithmic.RithmicInstrument
RithmicInstrumentProvider = _rithmic.RithmicInstrumentProvider
TimeBar = _rithmic.TimeBar
TimeInForce = _rithmic.TimeInForce
TradeTick = _rithmic.TradeTick

try:
    AccountEvent = _rithmic.AccountEvent
    PositionEvent = _rithmic.PositionEvent
except AttributeError:  # pragma: no cover - older compiled extensions

    class AccountEvent:  # type: ignore[no-redef]
        """Fallback placeholder when account events are not exported by the extension."""

    class PositionEvent:  # type: ignore[no-redef]
        """Fallback placeholder when position events are not exported by the extension."""


__all__ = [
    "AccountEvent",
    "ConnectionState",
    "ExecutionEvent",
    "MarketDataEvent",
    "OrderAccepted",
    "OrderCancelled",
    "OrderFilled",
    "OrderModified",
    "OrderRejected",
    "OrderSide",
    "OrderStatus",
    "OrderSubmitted",
    "OrderType",
    "PositionEvent",
    "QuoteTick",
    "RithmicDataClient",
    "RithmicExecutionClient",
    "RithmicGateway",
    "RithmicInstrument",
    "RithmicInstrumentProvider",
    "TimeBar",
    "TimeInForce",
    "TradeTick",
]

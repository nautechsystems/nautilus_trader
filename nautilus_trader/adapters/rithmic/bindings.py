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
    "TimeInForce",
    "TradeTick",
]

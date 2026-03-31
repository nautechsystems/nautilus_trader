from __future__ import annotations

from nautilus_trader.adapters.binance.http.error import classify_transport_error_type
from nautilus_trader.adapters.binance.http.error import is_transport_timeout_error
from nautilus_trader.core.nautilus_pyo3 import HttpTimeoutError


def test_timeout_classification_normalizes_python_and_http_timeout_errors() -> None:
    assert is_transport_timeout_error(TimeoutError("boom")) is True
    assert classify_transport_error_type(TimeoutError("boom")) == "TimeoutError"
    assert is_transport_timeout_error(HttpTimeoutError("boom")) is True
    assert classify_transport_error_type(HttpTimeoutError("boom")) == "TimeoutError"
    assert is_transport_timeout_error(RuntimeError("boom")) is False
    assert classify_transport_error_type(RuntimeError("boom")) is None

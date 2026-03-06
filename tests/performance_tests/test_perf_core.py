import pandas as pd

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import format_iso8601
from nautilus_trader.core.datetime import unix_nanos_to_iso8601


def test_nautilus_convert_to_snake_case(benchmark) -> None:
    benchmark(nautilus_pyo3.convert_to_snake_case, "PascalCase")


def test_unix_nanos_to_iso8601(benchmark) -> None:
    benchmark(lambda: unix_nanos_to_iso8601(0))


def test_format_iso8601(benchmark) -> None:
    dt = pd.Timestamp(0)

    benchmark(lambda: format_iso8601(dt))


def test_format_iso8601_millis(benchmark) -> None:
    dt = pd.Timestamp(0)

    benchmark(lambda: format_iso8601(dt, nanos_precision=False))

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT


def test_get_rate(benchmark):
    bid_quotes = {
        "BTC/USD": 11291.38,
        "ETH/USDT": 371.90,
        "XBT/USD": 11285.50,
    }

    ask_quotes = {
        "BTC/USD": 11292.58,
        "ETH/USDT": 372.11,
        "XBT/USD": 11286.0,
    }

    benchmark(
        nautilus_pyo3.get_exchange_rate,
        ETH.code,
        USDT.code,
        nautilus_pyo3.PriceType.MID,
        bid_quotes,
        ask_quotes,
    )

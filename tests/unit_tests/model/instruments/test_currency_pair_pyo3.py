from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_BTCUSDT = TestInstrumentProviderPyo3.btcusdt_binance()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.btcusdt_binance()
    item_2 = TestInstrumentProviderPyo3.btcusdt_binance()
    assert item_1 == item_2


def test_hash():
    assert hash(_BTCUSDT) == hash(_BTCUSDT)


def test_to_dict():
    result = _BTCUSDT.to_dict()
    assert nautilus_pyo3.CurrencyPair.from_dict(result) == _BTCUSDT
    assert result == {
        "type": "CurrencyPair",
        "id": "BTCUSDT.BINANCE",
        "raw_symbol": "BTCUSDT",
        "base_currency": "BTC",
        "quote_currency": "USDT",
        "price_precision": 2,
        "size_precision": 6,
        "price_increment": "0.01",
        "size_increment": "0.000001",
        "multiplier": "1",
        "lot_size": None,
        "max_quantity": "9000",
        "min_quantity": "0.00001",
        "max_notional": None,
        "min_notional": None,
        "min_price": "0.01",
        "max_price": "1000000",
        "maker_fee": "0.001",
        "margin_init": "0.0500",
        "margin_maint": "0.0250",
        "taker_fee": "0.001",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_pyo3_cython_conversion():
    currency_pair_pyo3 = TestInstrumentProviderPyo3.btcusdt_binance()
    currency_pair_pyo3_dict = currency_pair_pyo3.to_dict()
    currency_pair_cython = CurrencyPair.from_pyo3(currency_pair_pyo3)
    currency_pair_cython_dict = CurrencyPair.to_dict(currency_pair_cython)
    del currency_pair_cython_dict["tick_scheme_name"]  # TODO: Under development
    currency_pair_pyo3_back = nautilus_pyo3.CurrencyPair.from_dict(currency_pair_cython_dict)
    assert currency_pair_cython_dict == currency_pair_pyo3_dict
    assert currency_pair_pyo3 == currency_pair_pyo3_back

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import FuturesSpread
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_ES_FUTURES_SPREAD = TestInstrumentProviderPyo3.futures_spread_es()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.futures_spread_es()
    item_2 = TestInstrumentProviderPyo3.futures_spread_es()
    assert item_1 == item_2


def test_hash():
    assert hash(_ES_FUTURES_SPREAD) == hash(_ES_FUTURES_SPREAD)


def test_to_dict():
    result = _ES_FUTURES_SPREAD.to_dict()
    assert nautilus_pyo3.FuturesSpread.from_dict(result) == _ES_FUTURES_SPREAD
    assert result == {
        "type": "FuturesSpread",
        "id": "ESM4-ESU4.GLBX",
        "raw_symbol": "ESM4-ESU4",
        "asset_class": "INDEX",
        "underlying": "ES",
        "strategy_type": "EQ",
        "activation_ns": 1655818200000000000,
        "expiration_ns": 1718976600000000000,
        "currency": "USD",
        "price_precision": 2,
        "price_increment": "0.01",
        "size_increment": "1",
        "size_precision": 0,
        "multiplier": "1",
        "lot_size": "1",
        "max_price": None,
        "max_quantity": None,
        "min_price": None,
        "min_quantity": "1",
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0",
        "taker_fee": "0",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
        "exchange": "XCME",
    }


def test_legacy_futures_contract_from_pyo3():
    future = FuturesSpread.from_pyo3(_ES_FUTURES_SPREAD)

    assert future.id.value == "ESM4-ESU4.GLBX"


def test_pyo3_cython_conversion():
    futures_spread_pyo3 = TestInstrumentProviderPyo3.futures_spread_es()
    futures_spread_pyo3_dict = futures_spread_pyo3.to_dict()
    futures_spread_cython = FuturesSpread.from_pyo3(futures_spread_pyo3)
    futures_spread_cython_dict = FuturesSpread.to_dict(futures_spread_cython)
    del futures_spread_cython_dict["tick_scheme_name"]  # TODO: Under development
    futures_spread_pyo3_back = nautilus_pyo3.FuturesSpread.from_dict(futures_spread_cython_dict)
    assert futures_spread_pyo3_dict == futures_spread_cython_dict
    assert futures_spread_pyo3 == futures_spread_pyo3_back

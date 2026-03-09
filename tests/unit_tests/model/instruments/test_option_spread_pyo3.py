from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import OptionSpread
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_OPTION_SPREAD = TestInstrumentProviderPyo3.option_spread()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.option_spread()
    item_2 = TestInstrumentProviderPyo3.option_spread()
    assert item_1 == item_2


def test_hash():
    assert hash(_OPTION_SPREAD) == hash(_OPTION_SPREAD)


def test_to_dict():
    result = _OPTION_SPREAD.to_dict()
    assert nautilus_pyo3.OptionSpread.from_dict(result) == _OPTION_SPREAD
    assert result == {
        "type": "OptionSpread",
        "id": "UD:U$: GN 2534559.GLBX",
        "raw_symbol": "UD:U$: GN 2534559",
        "asset_class": "FX",
        "exchange": "XCME",
        "underlying": "SR3",
        "strategy_type": "GN",
        "activation_ns": 1699304047000000000,
        "expiration_ns": 1708729140000000000,
        "currency": "USDT",
        "price_precision": 2,
        "price_increment": "0.01",
        "size_increment": "1",
        "size_precision": 0,
        "multiplier": "1",
        "lot_size": "1",
        "max_quantity": None,
        "min_quantity": "1",
        "max_price": None,
        "min_price": None,
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0",
        "taker_fee": "0",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_legacy_option_contract_from_pyo3():
    option = OptionSpread.from_pyo3(_OPTION_SPREAD)

    assert option.id.value == "UD:U$: GN 2534559.GLBX"


def test_pyo3_cython_conversion():
    option_spread_pyo3 = TestInstrumentProviderPyo3.option_spread()
    option_spread_pyo3_dict = option_spread_pyo3.to_dict()
    option_spread_cython = OptionSpread.from_pyo3(option_spread_pyo3)
    option_spread_cython_dict = OptionSpread.to_dict(option_spread_cython)
    del option_spread_cython_dict["tick_scheme_name"]  # TODO: Under development
    option_spread_pyo3_back = nautilus_pyo3.OptionSpread.from_dict(option_spread_cython_dict)
    assert option_spread_cython_dict == option_spread_pyo3_dict
    assert option_spread_pyo3 == option_spread_pyo3_back

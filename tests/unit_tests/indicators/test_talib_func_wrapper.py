import importlib.util
import sys

import pytest


if importlib.util.find_spec("talib") is None:
    if sys.platform == "linux":
        # Raise the exception (expecting talib to be available on Linux)
        error_message = (
            "Failed to import TA-Lib. This module requires TA-Lib to be installed. "
            "Please visit https://github.com/TA-Lib/ta-lib-python for installation instructions. "
            "If TA-Lib is already installed, ensure it is correctly added to your Python environment."
        )
        raise ImportError(error_message)
    pytestmark = pytest.mark.skip(reason="talib is not installed")
else:
    import talib
    from talib import abstract

    from nautilus_trader.indicators.ta_lib.manager import TAFunctionWrapper


def test_init_with_valid_name_and_no_params():
    wrapper = TAFunctionWrapper(name="SMA")
    assert wrapper.name == "SMA"
    assert isinstance(wrapper.fn, talib._ta_lib.Function)
    assert isinstance(wrapper.output_names, list)
    assert all(isinstance(o, str) for o in wrapper.output_names)


def test_init_with_valid_name_and_params():
    wrapper = TAFunctionWrapper(name="EMA", params={"timeperiod": 10})
    assert wrapper.name == "EMA"
    assert wrapper.fn.parameters["timeperiod"] == 10


def test_repr():
    wrapper = TAFunctionWrapper(name="SMA", params={"timeperiod": 5})
    assert repr(wrapper) == "TAFunctionWrapper(SMA_5)"


def test_get_outputs_names():
    fn = abstract.Function("SMA")
    fn.set_parameters({"timeperiod": 5})
    output_names = TAFunctionWrapper._get_outputs_names("SMA", fn)
    assert output_names == ["SMA_5"]


def test_from_str_valid():
    wrapper = TAFunctionWrapper.from_str("SMA_5")
    assert wrapper.name == "SMA"
    assert wrapper.fn.parameters["timeperiod"] == 5


def test_from_str_invalid():
    with pytest.raises(Exception):
        TAFunctionWrapper.from_str("INVALID_5")


def test_from_list_of_str():
    indicators = ["SMA_5", "EMA_10"]
    wrappers = TAFunctionWrapper.from_list_of_str(indicators)
    assert len(wrappers) == 2
    assert all(isinstance(w, TAFunctionWrapper) for w in wrappers)

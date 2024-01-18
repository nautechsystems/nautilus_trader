# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

import importlib.util
import inspect
import sys
from unittest.mock import Mock

import numpy as np
import pytest

from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


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
    from nautilus_trader.indicators.ta_lib.manager import TAFunctionWrapper
    from nautilus_trader.indicators.ta_lib.manager import TALibIndicatorManager


@pytest.fixture(scope="session")
def bar_type() -> BarType:
    return BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL")


@pytest.fixture()
def indicator_manager(bar_type: BarType) -> TALibIndicatorManager:
    logger = Logger(
        level_stdout=LogLevel.INFO,
        bypass=True,
    )
    log = LoggerAdapter("TALibIndicatorManager", logger=logger)
    return TALibIndicatorManager(
        bar_type=bar_type,
        period=10,
        logger=log,
    )


@pytest.fixture()
def sample_bar_1(bar_type: BarType) -> Bar:
    return Bar(
        bar_type=bar_type,
        open=Price.from_str("1.57593"),
        high=Price.from_str("1.57614"),
        low=Price.from_str("1.57593"),
        close=Price.from_str("1.57610"),
        volume=Quantity.from_int(1_000_000),
        ts_event=1,
        ts_init=1,
    )


@pytest.fixture()
def sample_bar_1_update(bar_type: BarType) -> Bar:
    return Bar(
        bar_type=bar_type,
        open=Price.from_str("1.57593"),
        high=Price.from_str("1.57619"),
        low=Price.from_str("1.57593"),
        close=Price.from_str("1.57619"),
        volume=Quantity.from_int(2_000_000),
        ts_event=1,
        ts_init=1,
    )


@pytest.fixture()
def sample_bar_2(bar_type: BarType) -> Bar:
    return Bar(
        bar_type=bar_type,
        open=Price.from_str("1.57610"),
        high=Price.from_str("1.57621"),
        low=Price.from_str("1.57606"),
        close=Price.from_str("1.57608"),
        volume=Quantity.from_int(1_000_000),
        ts_event=2,
        ts_init=2,
    )


@pytest.fixture()
def sample_data(bar_type: BarType) -> list[Bar]:
    provider = TestDataProvider()
    instrument = TestInstrumentProvider.default_fx_ccy(
        symbol=bar_type.instrument_id.symbol.value,
        venue=bar_type.instrument_id.venue,
    )
    wrangler = BarDataWrangler(bar_type=bar_type, instrument=instrument)
    bars = wrangler.process(
        data=provider.read_csv_bars("fxcm/gbpusd-m1-bid-2012.csv")[:50],
    )
    return bars


def test_setup():
    # Arrange
    bar_type = BarType.from_str("EUR/USD.IDEALPRO-1-HOUR-MID-EXTERNAL")
    period = 10
    logger = Logger(
        level_stdout=LogLevel.INFO,
        bypass=True,
    )
    log = LoggerAdapter("TALibIndicatorManager", logger=logger)

    # Act
    indicator_manager = TALibIndicatorManager(
        bar_type=bar_type,
        period=period,
        logger=log,
    )

    # Assert
    assert indicator_manager.bar_type == bar_type
    assert indicator_manager.period == period


def test_invalid_bar_type():
    # Arrange, Act, Assert
    with pytest.raises(TypeError):
        TALibIndicatorManager(bar_type="invalid_bar_type", period=10)


def test_not_positive_period(bar_type):
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TALibIndicatorManager(bar_type=bar_type, period=0)


def test_not_positive_buffer_size(bar_type):
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TALibIndicatorManager(bar_type=bar_type, period=10, buffer_size=0)


def test_skip_uniform_price_bar_default_true(indicator_manager):
    # Assert
    indicator_manager._skip_uniform_price_bar = True


def test_skip_zero_close_bar_default_true(indicator_manager):
    # Assert
    indicator_manager._skip_zero_close_bar = True


def test_set_indicators(indicator_manager):
    # Arrange
    indicators = (TAFunctionWrapper("SMA", {"timeperiod": 50}), TAFunctionWrapper("MACD"))

    # Act
    indicator_manager.set_indicators(indicators)

    # Assert
    assert len(indicator_manager._indicators) == len(indicators)


def test_output_names_generation(indicator_manager):
    # Arrange
    indicators = (TAFunctionWrapper("SMA", {"timeperiod": 50}),)
    # Act
    indicator_manager.set_indicators(indicators)
    expected_output_names = indicator_manager.input_names() + indicators[0].output_names

    # Assert
    assert indicator_manager.output_names == tuple(expected_output_names)


def test_increment_count_correctly_increases_counter(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5

    # Act
    for i in range(2):
        indicator_manager._increment_count()

    # Assert
    assert indicator_manager.count == 2


def test_indicator_remains_uninitialized_with_insufficient_input_count(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5

    # Act
    for i in range(4):
        indicator_manager._increment_count()

    # Assert
    assert indicator_manager.has_inputs is True
    assert indicator_manager.initialized is False


def test_indicator_initializes_after_receiving_required_input_count(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5
    indicator_manager._update_ta_outputs = Mock()

    # Act
    for i in range(5):
        indicator_manager._increment_count()

    # Assert
    assert indicator_manager.has_inputs is True
    assert indicator_manager.initialized is True


def test_update_ta_outputs_default_append_is_true(indicator_manager):
    # Arrange, Act
    sig = inspect.signature(indicator_manager._update_ta_outputs)
    default_append = sig.parameters["append"].default

    # Assert
    assert default_append is True, "Default value for 'append' should be True"


def test_handle_bar_new(indicator_manager, sample_bar_1):
    # Arrange
    indicator_manager._update_ta_outputs = Mock()
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))
    expected = np.array(
        [
            (
                sample_bar_1.ts_event,
                sample_bar_1.ts_init,
                sample_bar_1.open.as_double(),
                sample_bar_1.high.as_double(),
                sample_bar_1.low.as_double(),
                sample_bar_1.close.as_double(),
                sample_bar_1.volume.as_double(),
            ),
        ],
        dtype=indicator_manager.input_dtypes(),
    )

    # Act
    indicator_manager.handle_bar(sample_bar_1)

    # Assert
    assert indicator_manager._input_deque[-1] == [expected]
    assert len(indicator_manager._input_deque) == 1
    assert indicator_manager.count == 1
    indicator_manager._update_ta_outputs.assert_called_once()


def test_handle_bar_update(indicator_manager, sample_bar_1, sample_bar_1_update):
    # Arrange
    indicator_manager._update_ta_outputs = Mock(side_effect=lambda append=True: None)
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))
    expected = np.array(
        [
            (
                sample_bar_1_update.ts_event,
                sample_bar_1_update.ts_init,
                sample_bar_1_update.open.as_double(),
                sample_bar_1_update.high.as_double(),
                sample_bar_1_update.low.as_double(),
                sample_bar_1_update.close.as_double(),
                sample_bar_1_update.volume.as_double(),
            ),
        ],
        dtype=indicator_manager.input_dtypes(),
    )

    # Act
    indicator_manager.handle_bar(sample_bar_1)
    indicator_manager.handle_bar(sample_bar_1_update)

    # Assert
    assert indicator_manager._input_deque[-1] == [expected]
    assert len(indicator_manager._input_deque) == 1
    assert indicator_manager.count == 1
    second_call_args, second_call_kwargs = indicator_manager._update_ta_outputs.call_args_list[1]
    assert (
        second_call_kwargs.get("append", None) is False
    ), "Second call was not made with append=False"


def test_handle_bar_out_of_sync(indicator_manager, sample_bar_1, sample_bar_2):
    # Arrange
    indicator_manager._update_ta_outputs = Mock(side_effect=lambda append=True: None)
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))
    expected = np.array(
        [
            (
                sample_bar_2.ts_event,
                sample_bar_2.ts_init,
                sample_bar_2.open.as_double(),
                sample_bar_2.high.as_double(),
                sample_bar_2.low.as_double(),
                sample_bar_2.close.as_double(),
                sample_bar_2.volume.as_double(),
            ),
        ],
        dtype=indicator_manager.input_dtypes(),
    )

    # Act
    indicator_manager.handle_bar(sample_bar_2)
    indicator_manager.handle_bar(sample_bar_1)  # <- old bar received later

    # Assert
    assert indicator_manager._input_deque[-1] == [expected]
    assert len(indicator_manager._input_deque) == 1
    assert indicator_manager.count == 1
    assert indicator_manager._data_error_counter == 1
    assert indicator_manager._last_ts_event == 2


def test_input_names():
    # Arrange
    expected = ["ts_event", "ts_init", "open", "high", "low", "close", "volume"]

    # Act, Assert
    assert TALibIndicatorManager.input_names() == expected


def test_input_dtypes():
    # Arrange
    expected = [
        ("ts_event", np.dtype("uint64")),
        ("ts_init", np.dtype("uint64")),
        ("open", np.dtype("float64")),
        ("high", np.dtype("float64")),
        ("low", np.dtype("float64")),
        ("close", np.dtype("float64")),
        ("volume", np.dtype("float64")),
    ]

    # Act, Assert
    assert TALibIndicatorManager.input_dtypes() == expected


def test_output_dtypes(indicator_manager):
    # Arrange
    expected = [
        ("ts_event", np.dtype("uint64")),
        ("ts_init", np.dtype("uint64")),
        ("open", np.dtype("float64")),
        ("high", np.dtype("float64")),
        ("low", np.dtype("float64")),
        ("close", np.dtype("float64")),
        ("volume", np.dtype("float64")),
        ("SMA_10", np.dtype("float64")),
    ]

    # Act
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))

    # Assert
    assert indicator_manager._output_dtypes == expected


def test_input_deque_maxlen_is_one_more_than_lookback(indicator_manager):
    # Arrange, Act
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))
    output_names = indicator_manager.input_names()
    lookback = 0
    for indicator in indicator_manager._indicators:
        output_names.extend(indicator.output_names)
        lookback = max(lookback, indicator.fn.lookback)
    expected_maxlen = lookback + 1

    # Assert
    assert indicator_manager._input_deque.maxlen == expected_maxlen


def test_stable_period_single_indicator(indicator_manager, sample_data):
    # Arrange, Act
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))
    expected = 19  # indicator_manager.period + lookback of sma_10

    # Assert
    assert indicator_manager._stable_period == expected


def test_stable_period_multiple_indicators(indicator_manager, sample_data):
    # Arrange, Act
    indicator_manager.set_indicators(
        TAFunctionWrapper.from_list_of_str(["SMA_10", "EMA_20", "ATR_14"]),
    )
    expected = 29  # indicator_manager.period + max lookback of all indicator

    # Assert
    assert indicator_manager._stable_period == expected


def test_output_array_when_not_initialized(indicator_manager, sample_data):
    # Arrange
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))

    # Act
    for bar in sample_data[:15]:
        indicator_manager.handle_bar(bar)

    # Assert
    assert indicator_manager.output_array is None


def test_output_array_multiple_output_indicator(indicator_manager, sample_data):
    # Arrange
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["MACD_12_26_9"]))
    expected_1 = np.array(
        [
            1.2788974555189014e-04,
            1.0112435988784974e-04,
            8.5582518167148791e-05,
            6.8531219604039961e-05,
            8.8447096904031852e-05,
            1.0224003011005678e-04,
            1.0908857559432938e-04,
            1.1207251438904997e-04,
            1.1158850602077663e-04,
            1.0202237800371883e-04,
        ],
    )
    expected_2 = np.array(
        [
            -6.1579287446791549e-05,
            -7.6999476312591649e-05,
            -8.4977673358046753e-05,
            -8.8624485371132294e-05,
            -5.3610638737606260e-05,
            -2.7077967138818465e-05,
            -1.3647902407444410e-05,
            -3.0235643303387811e-06,
            2.3138898333188349e-06,
            -2.1016908817234661e-06,
        ],
    )

    # Act
    for bar in sample_data[:45]:
        indicator_manager.handle_bar(bar)

    # Assert
    assert np.array_equal(indicator_manager.output_array["MACD_12_26_9"], expected_1)
    assert np.array_equal(indicator_manager.output_array["MACD_12_26_9_HIST"], expected_2)


def test_output_array_single_output_indicator(indicator_manager, sample_data):
    # Arrange
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))
    expected = np.array(
        [
            1.575571,
            1.5755810000000001,
            1.575589,
            1.575594,
            1.575593,
            1.575602,
            1.575633,
            1.575673,
            1.5757189999999999,
            1.575764,
        ],
    )

    # Act
    for bar in sample_data[:20]:
        indicator_manager.handle_bar(bar)

    # Assert
    assert np.array_equal(indicator_manager.output_array["SMA_10"], expected)


def test_value(indicator_manager, sample_data):
    # Arrange
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))

    # Act
    for bar in sample_data[:20]:
        indicator_manager.handle_bar(bar)

    # Assert
    assert indicator_manager.value("SMA_10") == 1.575764
    assert indicator_manager.value("SMA_10", 1) == 1.5757189999999999


def test_value_with_invalid_index(indicator_manager, sample_data):
    # Arrange
    indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(["SMA_10"]))

    # Act
    for bar in sample_data[:20]:
        indicator_manager.handle_bar(bar)

    # Assert
    with pytest.raises(ValueError):
        indicator_manager.value("SMA_10", 30)


def test_ohlcv_when_no_indicators_are_set(indicator_manager, sample_data):
    # Act
    for bar in sample_data[:20]:
        indicator_manager.handle_bar(bar)

    # Assert
    assert indicator_manager.output_array.shape == (10,)

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


try:
    import talib
    from talib import abstract
except ImportError as e:
    error_message = (
        "Failed to import TA-Lib. This module requires TA-Lib to be installed. "
        "Please visit https://github.com/TA-Lib/ta-lib-python for installation instructions. "
        "If TA-Lib is already installed, ensure it is correctly added to your Python environment."
    )
    raise ImportError(error_message) from e

import os
from collections import deque
from typing import Any

import numpy as np
import pandas as pd

from nautilus_trader.common.component import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators.base.indicator import Indicator
from nautilus_trader.indicators.ta_lib.common import output_suffix_map
from nautilus_trader.indicators.ta_lib.common import taf_params_re
from nautilus_trader.indicators.ta_lib.common import talib_indicator_manager_input_dtypes
from nautilus_trader.indicators.ta_lib.common import talib_indicator_manager_input_names
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType


class TAFunctionWrapper:
    """
    A wrapper class for TA-Lib functions, facilitating the handling of technical
    indicators.

    This class wraps around TA-Lib functions, allowing for easy manipulation and usage
    of various technical indicators. It stores the name of the indicator, its parameters,
    and the output names generated based on these parameters.

    Attributes
    ----------
    - name (str): The name of the technical indicator as defined in TA-Lib.
      For more information, visit https://ta-lib.github.io/ta-lib-python/
    - params (frozenset[tuple[str, int | float]]): A set of tuples representing
      the parameters for the technical indicator. Each tuple contains the parameter name
      and its value (either int or float). If unspecified, default parameters set by
      TA-Lib are used.
    - output_names (list[str]): A list of formatted output names for the technical indicator,
      generated based on the `name` and `params`.

    Notes
    -----
    The class utilizes TA-Lib, a popular technical analysis library, to handle the underlying
    functionality related to technical indicators.

    Example:
    -------
    - Creating an instance of TAFunctionWrapper:
      ```
      wrapper = TAFunctionWrapper(name="SMA", params={"timeperiod": 5})
      ```
      This will create a TAFunctionWrapper instance for the Simple Moving Average (SMA)
      indicator with a time period of 5.

    """

    def __init__(self, name: str, params: dict[str, int | float] | None = None) -> None:
        self.name = name
        self.fn = abstract.Function(name)
        self.fn.set_parameters(params or {})
        self.output_names = self._get_outputs_names(self.name, self.fn)

    def __repr__(self):
        return f"TAFunctionWrapper({','.join(map(str, self.output_names))})"

    def __eq__(self, other):
        return isinstance(other, TAFunctionWrapper) and self.output_names == other.output_names

    def __hash__(self):
        return hash(tuple(self.output_names))

    def __reduce__(self):
        return (self.from_str, (self.output_names[0],))

    @staticmethod
    def _get_outputs_names(name: str, fn: abstract.Function) -> list[str]:
        """
        Generate a list of output names for a given TA-Lib function and its parameters.

        This method constructs output names by appending a suffix based on the function's
        parameter values to the function's name. Each output name is also modified based on
        a predefined output_suffix_map. The generated names are converted to uppercase.

        Parameters
        ----------
        name : str
            The name of the TA-Lib function. This is used to identify the specific technical analysis
            function being used or referred to.
        fn : abstract.Function
            The TA-Lib function object. This object includes the function's parameters and output names,
            defining the behavior and output format of the function.

        Returns
        -------
        list[str]
            A list of formatted output names for the given function.

        """
        # Generate the suffix from the function's parameters
        suffix = "_".join(str(p) for p in fn.parameters.values())
        suffix = f"_{suffix}" if suffix else ""

        # Construct the output names
        output_names = [
            (name + suffix + output_suffix_map.get(output_name, f"_{output_name}")).upper()
            for output_name in fn.output_names
        ]

        return output_names

    @classmethod
    def from_str(cls, value: str) -> Any:
        """
        Construct an instance of the class based on a string representation of a TA-Lib
        function.

        This method parses the given string to identify the TA-Lib function name and its parameters.
        It then creates and configures the corresponding abstract.Function object. If the provided
        string matches one of the output names of the configured function, an instance of the class
        is returned. Otherwise, a ValueError is raised.

        Parameters
        ----------
        value : str
            The string representation of the TA-Lib function, which includes the function name and
            any parameters. This string is used to identify and configure the specific TA-Lib function
            for analysis or computation.

        Returns
        -------
        An instance of the class configured with the identified TA-Lib function and parameters.

        Raises
        ------
        ValueError
            If the string does not correspond to any output names of the configured function.

        Notes
        -----
        - The method relies on `talib.get_functions()` to retrieve available TA-Lib functions.
        - It uses a regular expression `taf_params_re` to find parameter values within the string.

        """
        name = ""
        for func_name in talib.get_functions():
            if value.startswith(func_name) and len(func_name) > len(name):
                name = func_name

        param_values = [
            float(num) if "." in num else int(num)
            for num in taf_params_re.findall(value.replace(name, ""))
        ]

        fn = abstract.Function(name)
        params = dict(zip(fn.parameters.keys(), param_values))
        fn.set_parameters(params)
        output_names = cls._get_outputs_names(name=name, fn=fn)
        if value in output_names:
            return cls(name=name, params=params)
        else:
            raise ValueError(f"{value=} not in {output_names=}")

    @classmethod
    def from_list_of_str(cls, indicators: list[str]) -> tuple[TAFunctionWrapper, ...]:
        """
        Create a tuple of TAFunctionWrapper instances from a list of indicator names.

        This class method filters out indicators that are already present in
        `talib_indicator_manager_input_names` and creates TAFunctionWrapper instances for
        the remaining indicators.

        Parameters
        ----------
        indicators : list[str]
            A list of string names representing technical indicators. These names correspond to
            specific technical indicators that are to be utilized or analyzed.

        Returns
        -------
        tuple[TAFunctionWrapper, ...]
            A tuple of TAFunctionWrapper instances created from the given list of indicator
            names, excluding any names present in `talib_indicator_manager_input_names`.

        """
        return tuple(
            cls.from_str(indicator)
            for indicator in indicators
            if indicator not in talib_indicator_manager_input_names
        )


class TALibIndicatorManager(Indicator):
    """
    Provides Numpy array for the TA based on given schema.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the instance data.
    period : int, default -1
        The period for the indicator.
    buffer_size : int, optional
        The buffer size for the indicator.
    skip_uniform_price_bar : bool, default True
        If uniform price bars should be skipped.
    skip_zero_close_bar : bool, default True
        If zero sized bars should be skipped.

    Raises
    ------
    ValueError
        If `indicators` is empty.
    ValueError
        If `period` is not positive (> 0).

    """

    def __init__(
        self,
        bar_type: BarType,
        period: int = 1,
        buffer_size: int | None = None,
        skip_uniform_price_bar: bool = True,
        skip_zero_close_bar: bool = True,
    ) -> None:
        super().__init__([])

        PyCondition().type(bar_type, BarType, "bar_type")
        PyCondition().positive_int(period, "period")
        if buffer_size is not None:
            PyCondition().positive_int(buffer_size, "buffer_size")

        self._log = Logger(name=type(self).__name__)

        # Initialize variables
        self._bar_type = bar_type
        self._period = period
        self._skip_uniform_price_bar = skip_uniform_price_bar
        self._skip_zero_close_bar = skip_zero_close_bar
        self._output_array: np.recarray | None = None
        self._last_ts_event: int = 0
        self._data_error_counter: int = 0
        self.count: int = 0
        self._output_deque: deque[np.ndarray] = deque(maxlen=buffer_size)

        # Initialize on `set_indicators`
        self._stable_period: int | None = None
        self._output_dtypes: list | None = None
        self._input_deque: deque[np.ndarray] | None = None
        self._indicators: set | None = None
        self.output_names: tuple | None = None

        # Initialize with empty indicators (acts as OHLCV placeholder in case no indicators are set)
        self.set_indicators(())

    def set_indicators(self, indicators: tuple[TAFunctionWrapper, ...]) -> None:
        """
        Set the indicators for the current instance and perform initialization steps.

        This method takes a tuple of TAFunctionWrapper objects, logs the action, and ensures
        that each element in the tuple is an instance of TAFunctionWrapper. It then updates
        the indicators, output names, stable period, input deque, and output data types
        for the current instance based on the provided indicators.

        Parameters
        ----------
        indicators : tuple[TAFunctionWrapper, ...]
            A tuple of TAFunctionWrapper instances. Each TAFunctionWrapper in the tuple is expected
            to have an 'output_names' attribute and a 'fn' object with a 'lookback' attribute. These
            are used to configure and calculate the technical indicators.

        The method performs the following steps:
        - Validates the type of each element in the 'indicators' tuple.
        - Sets the instance's indicators, ensuring uniqueness and maintaining order.
        - Calculates the maximum lookback period across all indicators.
        - Initializes the output names based on the indicators.
        - Updates the stable period based on the maximum lookback and the instance's period.
        - Initializes the input deque with a length equal to the stable period.
        - Sets the output data types, with special handling for the 'ts_event' column.

        This method also logs the setting and registration of indicators at the debug and
        info levels, respectively.

        """
        if self.initialized:
            self._log.info("Indicator already initialized. Skipping set_indicators.")
            return

        self._log.debug(f"Setting indicators {indicators}")
        PyCondition().list_type(list(indicators), TAFunctionWrapper, "ta_functions")

        self._indicators = set(indicators)
        output_names = list(self.input_names())
        lookback = 0

        for indicator in self._indicators:
            output_names.extend(indicator.output_names)
            lookback = max(lookback, indicator.fn.lookback)

        self._stable_period = lookback + self._period
        self._input_deque = deque(maxlen=lookback + 1)
        self.output_names = tuple(output_names)

        # Initialize the output dtypes
        self._output_dtypes = [
            (col, np.dtype("uint64") if col in ["ts_event", "ts_init"] else np.dtype("float64"))
            for col in self.output_names
        ]

        self._log.info(f"Registered {len(indicators)} indicators, {indicators}")

    def __repr__(self) -> str:
        return f"{self.name}[{self._bar_type}]"

    @staticmethod
    def input_names() -> list:
        return list(talib_indicator_manager_input_names)

    @staticmethod
    def input_dtypes() -> list[tuple[str, str]]:
        return list(talib_indicator_manager_input_dtypes)

    @property
    def name(self) -> str:
        return type(self).__name__

    @property
    def bar_type(self) -> BarType:
        return self._bar_type

    @property
    def period(self) -> int:
        return self._period

    def _update_ta_outputs(self, append: bool = True) -> None:
        """
        Update the output deque with calculated technical analysis indicators.

        This private method computes and updates the output values for technical
        analysis indicators based on the latest data in the input deque. It initializes
        a combined output array with base values (e.g., 'open', 'high', 'low', 'close',
        'volume', 'ts_event') from the most recent input deque entry. Each indicator's
        output is calculated and used to update the combined output array. The updated
        data is either appended to or replaces the latest entry in the output deque,
        depending on the value of the 'append' argument.

        Parameters
        ----------
        append : bool, default True
            Determines whether to append the new output to the output deque (True)
            or replace the most recent output (False).

        The method performs the following steps:
        - Initializes a combined output array with base values from the latest input
          deque entry.
        - Iterates through each indicator, calculates its output, and updates the
          combined output array.
        - Appends the combined output to the output deque or replaces its most recent
          entry based on the 'append' flag.
        - Resets the internal output array for reconstruction during the next access.

        This method logs actions at the debug level to track the calculation and
        updating process.

        """
        self._log.debug("Calculating outputs.")

        combined_output = np.zeros(1, dtype=self._output_dtypes)
        combined_output["ts_event"] = self._input_deque[-1]["ts_event"].item()
        combined_output["ts_init"] = self._input_deque[-1]["ts_init"].item()
        combined_output["open"] = self._input_deque[-1]["open"].item()
        combined_output["high"] = self._input_deque[-1]["high"].item()
        combined_output["low"] = self._input_deque[-1]["low"].item()
        combined_output["close"] = self._input_deque[-1]["close"].item()
        combined_output["volume"] = self._input_deque[-1]["volume"].item()

        input_array = np.concatenate(self._input_deque)
        for indicator in self._indicators:
            self._log.debug(f"Calculating {indicator.name} outputs.")
            inputs_dict = {name: input_array[name] for name in input_array.dtype.names}
            indicator.fn.set_input_arrays(inputs_dict)
            results = indicator.fn.run()

            if len(indicator.output_names) == 1:
                self._log.debug("Single output.")
                combined_output[indicator.output_names[0]] = results[-1]
            else:
                self._log.debug("Multiple outputs.")
                for i, output_name in enumerate(indicator.output_names):
                    combined_output[output_name] = results[i][-1]

        if append:
            self._log.debug("Appending output.")
            self._output_deque.append(combined_output)
        else:
            self._log.debug("Prepending output.")
            self._output_deque[-1] = combined_output

        # Reset output array to force rebuild on next access
        self._output_array = None

    def _increment_count(self) -> None:
        self.count += 1
        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self.count >= self._stable_period:
                self._set_initialized(True)
                self._log.info(f"Initialized with {self.count} bars")

    def value(self, name: str, index=0):
        """
        Retrieve the specified value from the output array based on the given indicator
        name and index.

        This method accesses the output array to fetch the value corresponding to a specific indicator
        identified by `name`. The `index` parameter determines which value to retrieve, with 0 referring
        to the latest value and any positive integer referring to older values (1 being the previous value,
        and so on). The method performs checks to ensure the name exists in the output names and that the
        indices are within valid ranges.

        Args:
        ----
        name : str
            The name of the indicator whose value is to be retrieved. This name should be one of the
            names specified in `self.output_names`.

        index : int, optional
            The index specifying which value to retrieve from the output array. The default value is 0,
            which indicates the latest value. A positive value accesses older data points, with higher
            values referring to progressively older data.

        Returns:
        -------
        The value of the specified indicator at the given index.

        Raises:
        ------
        ValueError
            If `name` is not in `self.output_names`, or if `index` or the internally
            calculated `translated_index` is negative, indicating an invalid index.

        Example:
        -------
        - To get the latest value of an indicator named 'EMA':
          ```
          latest_ema = instance.value('EMA_10')
          ```
        - To get the previous value of the same indicator:
          ```
          previous_ema = instance.value('EMA_10', 1)
          ```

        """
        PyCondition.is_in(name, self.output_names, "name", "output_names")
        PyCondition.not_negative(index, "index")

        translated_index = len(self.output_array) - index - 1
        PyCondition.not_negative(translated_index, "translated_index")

        return self.output_array[name][translated_index]

    @property
    def output_array(self) -> np.recarray | None:
        """
        Retrieve or generate the output array for the indicator.

        This method returns a record array (`np.recarray`) that contains the output data for the
        indicator. If the output array has not been generated previously (`self._output_array` is None),
        it calls `self.generate_output_array(truncate=True)` to generate the array. If the output array
        already exists, it uses the cached version and logs this action.

        Returns
        -------
        - np.recarray: A NumPy record array containing the output data for the indicator.

        Notes
        -----
        - The method uses lazy loading to generate the output array only when needed, enhancing
          performance by avoiding unnecessary recalculations.
        - The method ensures the use of a single instance of the output array, stored in
          `self._output_array`, to maintain consistency and reduce memory usage.

        Example
        -------
        - To access the output array of an indicator instance:
          ```
          output_data = indicator_instance.output_array()
          ```

        """
        if self._output_array is None:
            self._output_array = self.generate_output_array(truncate=True)
        else:
            self._log.debug("Using cached output array.")
        return self._output_array

    def generate_output_array(self, truncate: bool) -> np.recarray | None:
        """
        Generate the output array for the indicator, either truncated or complete.

        This method constructs a NumPy record array (`np.recarray`) from the accumulated outputs
        stored in `self._output_deque`. It can generate either a truncated array, containing only
        the data for the last `self.period` outputs, or a complete array with all accumulated data.
        The method also sets the output array to be non-writeable to preserve data integrity.

        Parameters
        ----------
        truncate : bool
            A flag indicating whether to truncate the output array to the size of `self.period`.
            If True, only the last `self.period` outputs are included in the array. If False,
            the entire contents of `self._output_deque` are used.

        Returns
        -------
        np.recarray or ``None``
            A NumPy record array containing the generated output data.
            The array is set as non-writeable to prevent accidental modifications.

        Note:
        ----
        - Truncating the array is useful for reducing memory usage and focusing on more recent data.
        - The method logs different messages based on whether the array is being truncated or not,
          aiding in debugging and monitoring.

        Example:
        -------
        - To generate a truncated output array:
          ```
          truncated_array = indicator_instance.generate_output_array(truncate=True)
          ```
        - To generate a complete output array (for instance at the end of backtest run):
          ```
          complete_array = indicator_instance.generate_output_array(truncate=False)
          ```

        """
        if not self.initialized:
            self._log.info("Indicator not initialized. Returning None.")
            return None

        if truncate:
            self._log.debug("Generating truncated output array.")
            output_array = np.concatenate(list(self._output_deque)[-self.period :])
        else:
            self._log.info("Generating complete output array.")
            output_array = np.concatenate(list(self._output_deque))

        # Make sure that the array is not writeable
        self._log.debug("Setting output array as not writeable.")
        output_array.flags.writeable = False
        return output_array

    @property
    def output_dataframe(self) -> pd.DataFrame:
        """
        Convert the output array to a pandas DataFrame.

        This method creates a pandas DataFrame from the output array generated by the indicator.
        It utilizes the `self.handle_dataframe` method to convert the NumPy record array
        (obtained from `self.output_array`) into a DataFrame format. The resulting DataFrame
        is useful for further data analysis or visualization.

        Returns
        -------
        pd.DataFrame
            A DataFrame representation of the indicator's output array.

        Notes
        -----
        - This method is a convenient way to interface with pandas, a popular data analysis
          library, allowing for more complex data manipulations and easier integration with
          other data processing workflows.

        Example
        -------
        - To get the output data of an indicator as a DataFrame:
          ```
          df = indicator_instance.output_dataframe()
          ```

        """
        return self.handle_dataframe(self.output_array)

    @staticmethod
    def handle_dataframe(
        array: np.recarray,
    ) -> pd.DataFrame:
        """
        Convert a NumPy record array into a pandas DataFrame with a datetime index.

        This method takes a NumPy record array and converts it into a pandas DataFrame. It processes
        the 'ts_event' field in the array to create a datetime index for the DataFrame, assuming
        'ts_event' represents timestamps in nanoseconds. These timestamps are converted to datetime
        objects in UTC, and then to a different timezone as per the 'TIME_ZONE' environment variable,
        if specified.

        Parameters
        ----------
        array : np.recarray
            The NumPy record array to be converted. It should have a 'ts_event' field representing
            timestamps in nanoseconds, along with other fields that will be columns in the DataFrame.

        Returns
        -------
        pd.DataFrame
            A DataFrame representation of the input array with a datetime index. The index is created
            from the 'ts_event' field in the array, and other fields in the array become columns in
            the DataFrame.

        Notes
        -----
        - The timezone conversion relies on the 'TIME_ZONE' environment variable. If it's not set,
          UTC is used as the default timezone.
        - This method is particularly useful for preparing time-series data for analysis or
          visualization in a more accessible and familiar format.

        Example
        -------
        - To convert a NumPy record array to a DataFrame with a datetime index:
          ```
          df = TAFunctionWrapper.handle_dataframe(record_array)
          ```

        """
        df = pd.DataFrame(array)
        df["datetime"] = pd.to_datetime(
            df["ts_event"],
            unit="ns",
            utc=True,
        ).dt.tz_convert(os.environ.get("TIME_ZONE", "UTC"))
        df = df.set_index("datetime")
        return df

    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar data.

        This method processes a single 'Bar' object to update the indicator's internal state.
        It validates the bar, checks if it's consistent with the expected bar type, and then
        incorporates its data into the indicator's calculations. The method handles bars with
        single price points and ensures that bars are processed in the correct chronological order.

        Parameters
        ----------
        bar : Bar
            The bar to update the indicator with. This should be an instance of 'Bar', containing
            essential data like timestamp, open, high, low, close, and volume. The bar is used to
            update the internal state of the indicator, and it is crucial that the bar data is
            provided in chronological order for accurate processing.

        Side Effects:
        - The internal state of the indicator is updated with the new bar data.
        - If the bar timestamp matches the last processed timestamp, the existing data is replaced.
        - If the bar timestamp is newer, it is appended and processed.
        - An error is logged and a counter is incremented if an out-of-sync bar is received.

        Notes
        -----
        - The method performs several checks to ensure data integrity, such as verifying the bar
          type and handling zero-value bars appropriately.

        Raises
        ------
        ValueError
            Raised if 'bar' is None or if 'bar.bar_type' does not match the expected type.

        """
        PyCondition.not_none(bar, "bar")
        PyCondition.equal(bar.bar_type, self._bar_type, "bar.bar_type", "self._bar_type")

        self._log.debug(f"Handling bar: {bar!r}")

        if self._skip_uniform_price_bar and bar.is_single_price():
            self._log.warning(f"Skipping uniform_price bar: {bar!r}")
            return
        if self._skip_zero_close_bar and bar.close.raw == 0:
            self._log.warning(f"Skipping zero close bar: {bar!r}")
            return

        bar_data = np.array(
            [
                (
                    bar.ts_event,
                    bar.ts_init,
                    bar.open.as_double(),
                    bar.high.as_double(),
                    bar.low.as_double(),
                    bar.close.as_double(),
                    bar.volume.as_double(),
                ),
            ],
            dtype=self.input_dtypes(),
        )

        if bar.ts_event == self._last_ts_event:
            self._input_deque[-1] = bar_data
            self._update_ta_outputs(append=False)
        elif bar.ts_event > self._last_ts_event:
            self._input_deque.append(bar_data)
            self._increment_count()
            self._update_ta_outputs()
        else:
            self._data_error_counter += 1
            self._log.error(f"Received out of sync bar: {bar!r}")
            return

        self._last_ts_event = bar.ts_event

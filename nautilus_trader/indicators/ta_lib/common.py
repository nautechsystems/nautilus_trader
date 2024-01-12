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

import re


# Tuple of data types for inputs to the TA-Lib indicator manager.
# Each element is a tuple containing the name of the input and its data type.
talib_indicator_manager_input_dtypes = (
    ("ts_event", "uint64"),
    ("ts_init", "uint64"),
    ("open", "float64"),
    ("high", "float64"),
    ("low", "float64"),
    ("close", "float64"),
    ("volume", "float64"),
)

# Extracting input names from talib_indicator_manager_input_dtypes for easy reference.
talib_indicator_manager_input_names = tuple(
    [item[0] for item in talib_indicator_manager_input_dtypes],
)

# Regular expression to find numerical parameters in TA function strings.
taf_params_re = re.compile(r"\d+\.\d+|\d+")

# Mapping of TA-Lib function names to specific output suffixes for naming consistency.
output_suffix_map = {
    "aroondown": "_down",
    "aroonup": "_up",
    "macd": "",
    "macdsignal": "_signal",
    "macdhist": "_hist",
    "mama": "",
    "minidx": "_min",
    "maxidx": "_max",
    "sine": "",
    "upperband": "_upper",
    "middleband": "_middle",
    "lowerband": "_lower",
    "integer": "",
    "real": "",
}

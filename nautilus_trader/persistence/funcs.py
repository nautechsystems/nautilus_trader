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

from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.core.nautilus_pyo3 import convert_to_snake_case
from nautilus_trader.model.identifiers import InstrumentId


INVALID_WINDOWS_CHARS = r'<>:"/\|?* '
CUSTOM_DATA_PREFIX = "custom_"

# Taken from https://github.com/dask/dask/blob/261bf174931580230717abca93fe172e166cc1e8/dask/utils.py
byte_sizes = {
    "kB": 10**3,
    "MB": 10**6,
    "GB": 10**9,
    "TB": 10**12,
    "PB": 10**15,
    "KiB": 2**10,
    "MiB": 2**20,
    "GiB": 2**30,
    "TiB": 2**40,
    "PiB": 2**50,
    "B": 1,
    "": 1,
}
byte_sizes = {k.lower(): v for k, v in byte_sizes.items()}
byte_sizes.update({k[0]: v for k, v in byte_sizes.items() if k and "i" not in k})
byte_sizes.update({k[:-1]: v for k, v in byte_sizes.items() if k and "i" in k})


def parse_bytes(s: float | str) -> int:
    if isinstance(s, int | float):
        return int(s)
    s = s.replace(" ", "")
    if not any(char.isdigit() for char in s):
        s = "1" + s

    i = 0
    for i in range(len(s) - 1, -1, -1):
        if not s[i].isalpha():
            break
    index = i + 1

    prefix = s[:index]
    suffix = s[index:]

    try:
        n = float(prefix)
    except ValueError as e:
        raise ValueError(f"Could not interpret '{prefix}' as a number") from e

    try:
        multiplier = byte_sizes[suffix.lower()]
    except KeyError as e:
        raise ValueError(f"Could not interpret '{suffix}' as a byte unit") from e

    result = n * multiplier
    return int(result)


def clean_windows_key(s: str) -> str:
    """
    Clean characters that are illegal on Windows from the string `s`.
    """
    for ch in INVALID_WINDOWS_CHARS:
        if ch in s:
            s = s.replace(ch, "-")
    return s


def class_to_filename(cls: type) -> str:
    """
    Convert the given class to a filename.
    """
    filename_mappings = {"OrderBookDeltas": "OrderBookDelta"}
    name = f"{convert_to_snake_case(filename_mappings.get(cls.__name__, cls.__name__))}"
    if not is_nautilus_class(cls):
        name = f"{CUSTOM_DATA_PREFIX}{name}"
    return name


def urisafe_instrument_id(instrument_id: InstrumentId | str) -> str:
    """
    Convert an instrument_id into a valid URI for writing to a file path.
    """
    if isinstance(instrument_id, InstrumentId):
        instrument_id = instrument_id.value
    return instrument_id.replace("/", "")


def combine_filters(*filters):
    filters = tuple(x for x in filters if x is not None)
    if len(filters) == 0:
        return
    elif len(filters) == 1:
        return filters[0]
    else:
        expr = filters[0]
        for f in filters[1:]:
            expr = expr & f
        return expr

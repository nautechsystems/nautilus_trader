# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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


CUSTOM_DATA_PREFIX = "custom_"


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
        return None
    elif len(filters) == 1:
        return filters[0]
    else:
        expr = filters[0]

        for f in filters[1:]:
            expr = expr & f

        return expr

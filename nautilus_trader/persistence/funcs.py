# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from pyarrow.dataset import Expression
from pyarrow.dataset import field

from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.core.nautilus_pyo3 import convert_to_snake_case
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.serialization.arrow.serializer import _ARROW_ENCODERS


CUSTOM_DATA_PREFIX = "custom_"


def class_to_filename(cls: type) -> str:
    """
    Convert the given class to a filename.
    """
    filename_mappings = {
        "OrderBookDelta": "OrderBookDeltas",
        "OrderBookDeltas": "OrderBookDeltas",
        "OrderBookDepth10": "OrderBookDepths",
    }
    name = f"{convert_to_snake_case(filename_mappings.get(cls.__name__, cls.__name__))}"

    if not is_nautilus_class(cls):
        name = f"{CUSTOM_DATA_PREFIX}{name}"

    return name


def filename_to_class(filename: str) -> type | None:
    """
    Convert the given filename back to a class.
    """
    builtin_filename_to_class = {
        "quote_tick": QuoteTick,
        "trade_tick": TradeTick,
        "bar": Bar,
        "order_book_deltas": OrderBookDelta,
        "order_book_depths": OrderBookDepth10,
        "mark_price_update": MarkPriceUpdate,
    }

    if filename in builtin_filename_to_class:
        return builtin_filename_to_class[filename]

    for data_cls in _ARROW_ENCODERS:
        if class_to_filename(data_cls) == filename:
            return data_cls

    return None


def urisafe_identifier(identifier: InstrumentId | BarType | str) -> str:
    """
    Convert an instrument_id into a valid URI for writing to a file path.
    """
    return str(identifier).replace("/", "")


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


def parse_filters_expr(s: str | None) -> Expression | None:
    """
    Parse a pyarrow.dataset filter expression from a string safely.

    >>> parse_filters_expr('field("Currency") == "CHF"')
    <pyarrow.dataset.Expression (Currency == "CHF")>

    >>> # Supports numeric comparisons and multi-character operators
    >>> parse_filters_expr('field("Price") >= 150.50')
    <pyarrow.dataset.Expression (Price >= 150.5)>

    >>> # Supports logical AND/OR with parentheses
    >>> parse_filters_expr('(field("Qty") > 0) & (field("Symbol") != "BTC")')
    <pyarrow.dataset.Expression ((Qty > 0) & (Symbol != "BTC"))>

    >>> # Returns None for empty input
    >>> parse_filters_expr(None)

    >>> # Fails on unauthorized Python code
    >>> parse_filters_expr("print('hello')")
    Traceback (most recent call last):
    ...
    ValueError: Filter expression 'print('hello')' is not allowed...

    """
    if not s:
        return None

    # Normalise single-quoted filters so our regex only has to reason about
    # the double-quoted form produced by Nautilus itself. If the expression
    # already contains double quotes we leave it unchanged to avoid corrupting
    # mixed quoting scenarios.
    if "'" in s and '"' not in s:
        s = s.replace("'", '"')

    # Security: Only allow very specific PyArrow field expressions
    # REGEX COMPONENTS
    # f_part: matches field("name")
    f_part = r'field\("[^"]+"\)'
    # o_part: matches ==, !=, <, <=, >, >=
    o_part = r"[!=<>]{1,2}"
    # v_part: matches "string", integers (100), or floats (100.5)
    v_part = r'("[^"]*"|\d+(\.\d+)?)'

    # Atomic comparison pattern: optional parens + field + op + value
    comparison = rf"(\()?{f_part}\s*{o_part}\s*{v_part}(\))?"

    # Full pattern: allows recursive chains of & (AND) or | (OR)
    safe_pattern = rf"^{comparison}(\s*[|&]\s*{comparison})*$"

    if not re.match(safe_pattern, s.strip()):
        raise ValueError(
            f"Filter expression '{s}' is not allowed. "
            "Only field() comparisons with strings or numbers are permitted.",
        )

    try:
        # For now, rely on the regex validation above to guarantee safety and
        # evaluate the expression in a minimal global namespace that only exposes
        # the `field` helper. Built-ins are intentionally left untouched because
        # PyArrow requires access to them (for example it imports `decimal` under
        # the hood). Stripping them leads to a hard crash inside the C++ layer
        # of Arrow. The expression is still safe because the regex prevents any
        # reference other than the allowed `field(...)` comparisons.
        allowed_globals = {"field": field}
        return eval(s, allowed_globals, {})  # noqa: S307

    except Exception as e:
        raise ValueError(f"Failed to parse filter expression '{s}': {e}")

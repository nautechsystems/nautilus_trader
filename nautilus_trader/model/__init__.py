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
"""
The `model` subpackage defines a rich trading domain model.

The domain model is agnostic of any system design, seeking to represent the logic and
state transitions of trading in a generic way. Many system implementations could be
built around this domain model.

"""

from typing import Union

from nautilus_trader.core import nautilus_pyo3


NAUTILUS_PYO3_DATA_TYPES: tuple[type, ...] = (
    nautilus_pyo3.OrderBookDelta,
    nautilus_pyo3.OrderBookDepth10,
    nautilus_pyo3.QuoteTick,
    nautilus_pyo3.TradeTick,
    nautilus_pyo3.Bar,
)

NautilusRustDataType = Union[  # noqa: UP007 (mypy does not like pipe operators)
    nautilus_pyo3.OrderBookDelta,
    nautilus_pyo3.OrderBookDepth10,
    nautilus_pyo3.QuoteTick,
    nautilus_pyo3.TradeTick,
    nautilus_pyo3.Bar,
]

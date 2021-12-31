# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick


cdef class Indicator:
    """
    The abstract base class for all indicators.

    Parameters
    ----------
    params : list
        The initialization parameters for the indicator.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, list params not None):
        self._params = params.copy()

        self.name = type(self).__name__
        self.has_inputs = False
        self.initialized = False

    def __repr__(self) -> str:
        return f"{self.name}({self._params_str()})"

    cdef str _params_str(self):
        return str(self._params)[1:-1].replace("'", '') if self._params else ''

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError(f"Cannot handle {repr(tick)}: method not implemented in subclass")  # pragma: no cover

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError(f"Cannot handle {repr(tick)}: method not implemented in subclass")  # pragma: no cover

    cpdef void handle_bar(self, Bar bar) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError(f"Cannot handle {repr(bar)}: method not implemented in subclass")  # pragma: no cover

    cpdef void reset(self) except *:
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._reset()
        self.has_inputs = False
        self.initialized = False

    cpdef void _set_has_inputs(self, bint setting) except *:
        self.has_inputs = setting

    cpdef void _set_initialized(self, bint setting) except *:
        self.initialized = setting

    cpdef void _reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

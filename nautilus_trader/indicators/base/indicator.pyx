# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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


cdef class Indicator:
    """
    The base class for all indicators.
    """

    def __init__(self, list params not None):
        """
        Initialize a new instance of the `Indicator` class.

        Parameters
        ----------
        params : list
            The initialization parameters for the indicator.

        """
        self._name = type(self).__name__
        self._params = params
        self._has_inputs = False
        self._initialized = False

    def __repr__(self) -> str:
        return f"{self.name}({self._params_str()})"

    @property
    def name(self):
        """
        The name of the indicator.

        Returns
        -------
        str

        """
        return self._name

    @property
    def params(self):
        """
        The indicators parameter values.

        Returns
        -------
        str

        """
        return self._params.copy()

    @property
    def has_inputs(self):
        """
        If the indicator has received inputs.

        Returns
        -------
        bool
            True if inputs received, else False.

        """
        return self._has_inputs

    @property
    def initialized(self):
        """
        If the indicator is warmed up and initialized.

        Returns
        -------
        bool
            True if initialized, else False.

        """
        return self._initialized

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """Abstract method."""
        raise NotImplementedError(f"Cannot handle {repr(tick)}, method not implemented in subclass")

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """Abstract method."""
        raise NotImplementedError(f"Cannot handle {repr(tick)}, method not implemented in subclass")

    cpdef void handle_bar(self, Bar bar) except *:
        """Abstract method."""
        raise NotImplementedError(f"Cannot handle {repr(bar)}, method not implemented in subclass")

    cpdef void reset(self) except *:
        # Override should call _reset_base()
        raise NotImplemented("method must be implemented in the subclass")

    cdef str _params_str(self):
        return str(self._params)[1:-1].replace("'", '').strip('()') if self._params else ''

    cdef void _set_has_inputs(self, bint setting) except *:
        self._has_inputs = setting

    cdef void _set_initialized(self, bint setting) except *:
        self._initialized = setting

    cdef void _reset_base(self) except *:
        self._has_inputs = False
        self._initialized = False

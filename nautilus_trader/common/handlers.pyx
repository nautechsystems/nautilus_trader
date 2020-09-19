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

from cpython.object cimport PyObject_Repr

from nautilus_trader.core.correctness cimport Condition


cdef class Handler:
    """
    The base class for all handlers.
    """
    def __init__(self, handler not None: callable):
        """
        Initialize a new instance of the Handler class.

        :param handler: The callable handler.
        :raises TypeError: If handler is not of type Callable.
        """
        Condition.callable(handler, "handler")

        self.handle = handler

    def __eq__(self, Handler other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return PyObject_Repr(self.handle) == PyObject_Repr(other.handle)

    def __ne__(self, Handler other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self.handle != other.handle

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.handle)


cdef class QuoteTickHandler(Handler):
    """
    Provides a handler for quote tick objects.
    """

    def __init__(self, handler not None: callable):
        """
        Initialize a new instance of the QuoteTickHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)


cdef class TradeTickHandler(Handler):
    """
    Provides a handler for trade tick objects.
    """

    def __init__(self, handler not None: callable):
        """
        Initialize a new instance of the TradeTickHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)


cdef class BarHandler(Handler):
    """
    Provides a handler for bar type and bar objects.
    """

    def __init__(self, handler not None: callable):
        """
        Initialize a new instance of the BarHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)


cdef class InstrumentHandler(Handler):
    """
    Provides a handler for instrument objects.
    """

    def __init__(self, handler not None: callable):
        """
        Initialize a new instance of the InstrumentHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)


cdef class EventHandler(Handler):
    """
    Provides a handler for event objects.
    """

    def __init__(self, handler not None: callable):
        """
        Initialize a new instance of the EventHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)

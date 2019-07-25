#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="handlers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from typing import Callable

from nautilus_trader.core.precondition cimport Precondition
from nautilus_trader.model.objects cimport Tick, BarType, Bar


cdef class Handler:
    """
    The base class for all handlers.
    """
    def __init__(self, handler: Callable):
        """
        Initializes a new instance of the TickHandler class.

        :param handler: The callable handler.
        """
        Precondition.type(handler, Callable, 'handler')

        self.handle = handler

    def __eq__(self, Handler other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.handle == other.handle

    def __ne__(self, Handler other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.handle != other.handle

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self.handle)


cdef class TickHandler(Handler):
    """
    Provides a handler for tick objects.
    """

    def __init__(self, handler: Callable):
        """
        Initializes a new instance of the TickHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)


cdef class BarHandler(Handler):
    """
    Provides a handler for bar type and bar objects.
    """

    def __init__(self, handler: Callable):
        """
        Initializes a new instance of the BarHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)


cdef class EventHandler(Handler):
    """
    Provides a handler for event objects.
    """

    def __init__(self, handler: Callable):
        """
        Initializes a new instance of the EventHandler class.

        :param handler: The callable handler.
        """
        super().__init__(handler)

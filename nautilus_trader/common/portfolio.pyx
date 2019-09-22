# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.objects cimport Money
from nautilus_trader.common.logger cimport Logger, LoggerAdapter


cdef class Portfolio:
    """
    Provides a trading portfolio of positions.
    """

    def __init__(self,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger=None):
        """
        Initializes a new instance of the Portfolio class.

        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)

    cpdef void reset(self):
        """
        Reset the portfolio by returning all stateful values to their initial value.
        """
        self._log.info(f"Resetting...")

        self._log.info("Reset.")

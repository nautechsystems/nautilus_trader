#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="fxcm.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.model.enums import Venue
from inv_trader.model.objects import Symbol


# noinspection PyPep8Naming
class FXCMSymbols:
    """
    Provides a factory for creating FXCM symbols.
    """

    @staticmethod
    def AUDUSD():
        """
        :return: The AUDUSD.FXCM symbol.
        """
        return Symbol("AUDUSD", Venue.FXCM)

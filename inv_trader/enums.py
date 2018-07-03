#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="enums.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from enum import Enum


class Venue(Enum):
    DUKASCOPY = 0,
    FXCM = 1


class Resolution(Enum):
    TICK = 0,
    SECOND = 1,
    MINUTE = 2,
    HOUR = 3,
    DAY = 4,


class QuoteType(Enum):
    BID = 0,
    ASK = 1,
    LAST = 2,
    MID = 3,

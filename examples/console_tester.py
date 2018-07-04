#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_console.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.data import LiveDataClient
from inv_trader.enums import Venue

if __name__ == "__main__":
    client = LiveDataClient()
    print(client.connect())
    print(client.subscribe_tick_data('audusd', Venue.FXCM))

    input("Press Enter to continue...")

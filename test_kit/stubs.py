#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="constants.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pytz

from datetime import datetime, timedelta

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970.
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC)


class TestStubs(object):

    @staticmethod
    def unix_epoch(offset_mins: int=0) -> datetime:
        """
        Generate a stub datetime based on the given offset from Unix epoch time.

        Unix time (also known as POSIX time or epoch time) is a system for
        describing instants in time, defined as the number of seconds that have
        elapsed since 00:00:00 Coordinated Universal Time (UTC), on Thursday,
        1 January 1970, minus the number of leap seconds which have taken place
        since then.

        :return: The unix epoch datetime plus any offset.
        """
        return UNIX_EPOCH + timedelta(minutes=offset_mins)

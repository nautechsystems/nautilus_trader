# -------------------------------------------------------------------------------------------------
# <copyright file="message_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum MessageType:
    UNKNOWN = -1,
    COMMAND = 0,
    EVENT = 1,
    REQUEST = 2,
    RESPONSE = 3

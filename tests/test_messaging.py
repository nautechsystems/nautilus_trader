#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import msgpack


class MessagingTests(unittest.TestCase):

    def test_can_serialize_event(self):
        # Arrange
        message = {
            'header': 'order_cancelled',
            'symbol': 'AUDUSD.FXCM',
            'order_id': 'O123456',
            'timestamp': '1970-01-01T00:00:00.000Z'}


        # Act
        packed = msgpack.packb(message)

        # Assert
        print(message)
        print(packed)

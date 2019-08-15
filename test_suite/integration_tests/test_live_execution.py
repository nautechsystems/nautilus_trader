# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import unittest

from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.core.types import GUID
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import OrderId, ExecutionId, ExecutionTicket
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price
from nautilus_trader.live.execution import ExecutionDatabase
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))

# Requirements:
#    - A Redis instance listening on the default port 6379


class ExecutionDatabaseTests(unittest.TestCase):

    # These tests require a Redis instance listening on the default port 6379

    def setUp(self):
        # Fixture Setup

        self.trader_id = TraderId('000')
        self.store = ExecutionDatabase(trader_id=self.trader_id)

    def test_can_store_order_event(self):
        # Arrange
        order_id = OrderId('O-201908160323-999-001')

        event = OrderFilled(
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.store.store(event)

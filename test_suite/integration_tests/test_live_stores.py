# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_stores.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import unittest

from nautilus_trader.common.logger import LogLevel
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.network.responses import MessageReceived
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer, MsgPackResponseSerializer
from nautilus_trader.live.execution import LiveExecClient
from nautilus_trader.live.stores import EventStore
from test_kit.stubs import TestStubs
from test_kit.mocks import MockCommandRouter, MockPublisher
from test_kit.strategies import TestStrategy1
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.account import Account
from nautilus_trader.common.brokerage import CommissionCalculator
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.core.types import GUID
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import IdTag, OrderId, PositionId, ExecutionId, \
    ExecutionTicket
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price, Money
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.trade.portfolio import Portfolio
from nautilus_trader.trade.strategy import TradingStrategy

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"


class EventStoreTests(unittest.TestCase):

    # These tests require a Redis instance listening on the default port 6379

    def setUp(self):
        # Fixture Setup

        self.trader_id = TraderId('999')
        self.store = EventStore(trader_id=self.trader_id)

    def test_can_store_order_event(self):
        # Arrange
        print(LogLevel.CRITICAL)
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

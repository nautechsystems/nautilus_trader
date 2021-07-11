# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest

from nautilus_trader.cache.database import CacheDatabase
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.identifiers import StrategyId
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.audusd_id()
GBPUSD_SIM = TestStubs.gbpusd_id()


class TestCacheDatabase:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

        self.database = CacheDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

    def test_flush_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.flush()

    def test_load_currencies_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_currencies()

    def test_load_instruments_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_instruments()

    def test_load_accounts_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_accounts()

    def test_load_orders_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_orders()

    def test_load_positions_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_positions()

    def test_load_currency_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_currency(None)

    def test_load_instrument_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_instrument(None)

    def test_load_account_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_account(None)

    def test_load_order_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_order(None)

    def test_load_position_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_position(None)

    def test_load_strategy_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.load_strategy(None)

    def test_delete_strategy_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.delete_strategy(None)

    def test_add_account_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.add_account(None)

    def test_add_order_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.add_order(None)

    def test_add_position_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.add_position(None)

    def test_update_account_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.update_account(None)

    def test_update_order_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.update_order(None)

    def test_update_position_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.update_position(None)

    def test_update_strategy_when_not_implemented_raises_exception(self):
        with pytest.raises(NotImplementedError):
            self.database.update_strategy(None)

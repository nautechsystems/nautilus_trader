# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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


from nautilus_trader.accounting.manager import AccountsManager
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.execution import TestExecStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestAccountingManager:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
        )
        self.cache = TestComponentStubs.cache()
        self.instrument = AUDUSD_SIM
        self.account = TestExecStubs.cash_account()
        self.manager = AccountsManager(
            cache=self.cache, log=LoggerAdapter("AccountManager", self.logger), clock=self.clock
        )

    # def test_update_balance_lock_no_orders(self):
    #     self.manager.update_orders(
    #         account=self.account,
    #         instrument=self.instrument,
    #         orders_open=[]
    #     )

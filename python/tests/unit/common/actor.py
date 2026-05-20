# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Minimal test fixtures for LiveNode from_config registration tests.

PyO3 #[new] maps to __new__, not __init__. Subclasses inherit the base constructor
automatically and should not define __init__.

"""

from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.core import UUID4
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy


class TestActorConfig(DataActorConfig):
    pass


class TestActor(DataActor):
    pass


class TestStrategy(Strategy):
    pass


class PortfolioProbeStrategy(Strategy):
    observed_portfolio = None
    observed_account = None
    observed_equity_by_venue = None
    observed_equity_by_account = None
    observed_initialized = None

    def on_start(self):
        portfolio = self.portfolio
        account = portfolio.account(venue=Venue("SIM"))

        type(self).observed_portfolio = portfolio
        type(self).observed_account = account
        type(self).observed_initialized = portfolio.is_initialized()
        type(self).observed_equity_by_venue = portfolio.equity(venue=Venue("SIM"))
        type(self).observed_equity_by_account = portfolio.equity(account_id=account.id)


def _market_order(
    strategy: Strategy,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: Quantity,
) -> MarketOrder:
    return MarketOrder(
        trader_id=strategy.trader_id,
        strategy_id=strategy.strategy_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(f"{strategy.strategy_id}-{UUID4()}"),
        order_side=side,
        quantity=quantity,
        init_id=UUID4(),
        ts_init=strategy.clock.timestamp_ns(),
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )


class PortfolioHedgedProbeStrategy(Strategy):
    observed_portfolio = None
    observed_account = None

    def on_start(self):
        self._instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        self._quote_count = 0
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, tick):
        if self._quote_count == 0:
            self.submit_order(
                _market_order(
                    self,
                    self._instrument_id,
                    OrderSide.BUY,
                    Quantity.from_str("100000"),
                ),
            )
        elif self._quote_count == 1:
            self.submit_order(
                _market_order(
                    self,
                    self._instrument_id,
                    OrderSide.SELL,
                    Quantity.from_str("100000"),
                ),
            )
        self._quote_count += 1

    def on_stop(self):
        portfolio = self.portfolio
        account = portfolio.account(venue=Venue("SIM"))

        type(self).observed_portfolio = portfolio
        type(self).observed_account = account


class TestExecAlgorithmConfig(DataActorConfig):
    pass


class TestExecAlgorithm(DataActor):
    pass

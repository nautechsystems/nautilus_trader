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
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import Venue
from nautilus_trader.trading import Controller
from nautilus_trader.trading import ImportableStrategyConfig
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class TestActorConfig(DataActorConfig):
    pass


class TestActor(DataActor):
    pass


class TestStrategy(Strategy):
    pass


class FailingStartStrategy(Strategy):
    def on_start(self):
        raise RuntimeError("simulated live node strategy start failure")


class LifecycleProbeStrategy(Strategy):
    started = 0
    stopped = 0
    disposed = 0

    @classmethod
    def reset(cls):
        cls.started = 0
        cls.stopped = 0
        cls.disposed = 0

    def on_start(self):
        type(self).started += 1

    def on_stop(self):
        type(self).stopped += 1

    def on_dispose(self):
        type(self).disposed += 1


class TestStrategyConfig(StrategyConfig):
    def __new__(cls, *args, strategy_id: str | None = None, **kwargs):
        instance = super().__new__(cls, *args, **kwargs)
        instance._strategy_id_override = strategy_id
        return instance

    def __init__(self, strategy_id: str | None = None, **kwargs):
        super().__init__()

    @property
    def strategy_id(self):
        if self._strategy_id_override is not None:
            return self._strategy_id_override
        return super().strategy_id


class TestControllerConfig(DataActorConfig):
    pass


class ControllerRegistrationProbeConfig(DataActorConfig):
    def __init__(self, actor_id=None, log_events: bool = True, log_commands: bool = True):
        self.actor_id = actor_id
        self.log_events = log_events
        self.log_commands = log_commands


class ControllerRegistrationProbe(Controller):
    constructed = 0
    received_actor_id = None

    @classmethod
    def reset(cls):
        cls.constructed = 0
        cls.received_actor_id = None

    def __init__(self, config):
        super().__init__(config)
        type(self).constructed += 1
        type(self).received_actor_id = str(config.actor_id)


class ControllerCreatedStrategy(Strategy):
    started = 0

    @classmethod
    def reset(cls):
        cls.started = 0

    def on_start(self):
        type(self).started += 1


class StrategyCreatingController(Controller):
    started = 0
    created_strategy_id = None

    @classmethod
    def reset(cls):
        cls.started = 0
        cls.created_strategy_id = None

    def on_start(self):
        type(self).started += 1
        type(self).created_strategy_id = self.create_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:ControllerCreatedStrategy",
                config_path="tests.unit.common.actor:TestStrategyConfig",
                config={"strategy_id": "ControllerCreatedStrategy-001"},
            ),
        )


class NonStartingStrategyCreatingController(StrategyCreatingController):
    def on_start(self):
        type(self).started += 1
        type(self).created_strategy_id = self.create_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:ControllerCreatedStrategy",
                config_path="tests.unit.common.actor:TestStrategyConfig",
                config={"strategy_id": "ControllerCreatedStrategy-001"},
            ),
            start=False,
        )


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


class OrderFactoryProbeStrategy(Strategy):
    observed_order = None
    observed_invalid_order_error = None
    observed_next_client_order_id = None
    observed_client_order_id_count = None
    observed_order_list_id_count = None

    def on_start(self):
        order_factory = self.order_factory

        try:
            order_factory.market(
                InstrumentId.from_str("AUD/USD.SIM"),
                OrderSide.BUY,
                Quantity.from_str("100000"),
                time_in_force=TimeInForce.GTD,
            )
        except ValueError as e:
            type(self).observed_invalid_order_error = str(e)

        type(self).observed_order = order_factory.market(
            InstrumentId.from_str("AUD/USD.SIM"),
            OrderSide.BUY,
            Quantity.from_str("100000"),
        )
        type(self).observed_next_client_order_id = self.order_factory.generate_client_order_id()
        type(self).observed_client_order_id_count = order_factory.get_client_order_id_count()
        type(self).observed_order_list_id_count = order_factory.get_order_list_id_count()


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


class OrderListCacheProbeStrategy(Strategy):
    observed_order_list = None
    observed_order_lists = None
    observed_order_list_id = None
    observed_client_order_ids = None
    observed_strategy_id = None

    @classmethod
    def reset(cls):
        cls.observed_order_list = None
        cls.observed_order_lists = None
        cls.observed_order_list_id = None
        cls.observed_client_order_ids = None
        cls.observed_strategy_id = None

    def on_start(self):
        instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        orders = self.order_factory.bracket(
            instrument_id=instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("100000"),
            tp_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("0.90000"),
        )

        type(self).observed_client_order_ids = [order.client_order_id for order in orders]
        type(self).observed_strategy_id = self.strategy_id
        self.submit_order_list(orders)

        order_lists = self.cache.order_lists(
            instrument_id=instrument_id,
            strategy_id=self.strategy_id,
        )
        cached = order_lists[0]
        type(self).observed_order_lists = order_lists
        type(self).observed_order_list_id = cached.id
        type(self).observed_order_list = self.cache.order_list(cached.id)


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

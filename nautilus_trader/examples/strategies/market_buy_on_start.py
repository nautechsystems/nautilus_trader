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
Strategy that submits a single large market BUY on the first quote.

Used by liquidation engine tests to open an underwater position so the exchange's
liquidation logic can be exercised end-to-end.

This strategy uses the pyo3-native Strategy/StrategyConfig base classes so it can be
loaded by the Rust BacktestEngine via ImportableStrategyConfig.

"""

from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3.model import ClientOrderId
from nautilus_trader.core.nautilus_pyo3.model import ContingencyType
from nautilus_trader.core.nautilus_pyo3.model import InstrumentId
from nautilus_trader.core.nautilus_pyo3.model import MarketOrder
from nautilus_trader.core.nautilus_pyo3.model import OrderSide
from nautilus_trader.core.nautilus_pyo3.model import Quantity
from nautilus_trader.core.nautilus_pyo3.model import TimeInForce
from nautilus_trader.core.nautilus_pyo3.trading import Strategy
from nautilus_trader.core.nautilus_pyo3.trading import StrategyConfig


class MarketBuyOnStartConfig(StrategyConfig):
    """
    Configuration for ``MarketBuyOnStart``.

    Parameters
    ----------
    instrument_id : str
        The instrument ID string to trade.
    trade_size : int
        The number of contracts to buy on the first quote.

    """

    def __new__(cls, *args, **kwargs):
        # pyo3 StrategyConfig.__new__ only accepts its own known kwargs.
        # Strip our custom fields before delegating so base validation passes.
        kwargs.pop("instrument_id", None)
        kwargs.pop("trade_size", None)
        kwargs.pop("rebuy_after_close", None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        trade_size: int,
        rebuy_after_close: bool = False,
        **kwargs,
    ) -> None:
        super().__init__()
        self.instrument_id = instrument_id
        self.trade_size = int(trade_size)
        self.rebuy_after_close = bool(rebuy_after_close)


class MarketBuyOnStart(Strategy):
    """
    Opens a single large market BUY on the first received quote.

    Useful for liquidation tests: it enters a position at the first (healthy)
    price, then the test data feeds a crash price that should trigger the
    exchange's liquidation engine.

    Parameters
    ----------
    config : MarketBuyOnStartConfig
        The strategy configuration.

    """

    def __init__(self, config: MarketBuyOnStartConfig) -> None:
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._trade_size = Quantity.from_int(config.trade_size)
        self._rebuy_after_close = config.rebuy_after_close
        self._bought = False
        self._order_count = 0

    def on_start(self) -> None:
        self.subscribe_quotes(self._instrument_id)

    def on_quote(self, quote) -> None:
        if self._bought:
            return
        self._bought = True
        self._order_count += 1
        order = MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=self._instrument_id,
            client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
            order_side=OrderSide.BUY,
            quantity=self._trade_size,
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            reduce_only=False,
            quote_quantity=False,
            contingency_type=ContingencyType.NO_CONTINGENCY,
        )
        self.submit_order(order)

    def on_reset(self) -> None:
        self._bought = False
        self._order_count = 0

    def on_position_closed(self, event) -> None:
        if self._rebuy_after_close:
            self._bought = False

    def on_stop(self) -> None:
        pass

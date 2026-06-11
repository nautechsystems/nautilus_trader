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
EMA cross strategy routing orders through the TWAP execution algorithm.

Identical to ``EMACross`` except entries are submitted with an
``exec_algorithm_id`` so the engine routes them to a registered TWAP
execution algorithm for slicing.

"""

from __future__ import annotations

from strategies.ema_cross import EMACross
from strategies.ema_cross import EMACrossConfig

from nautilus_trader.core import UUID4
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import TimeInForce


class EMACrossTWAPConfig(EMACrossConfig):
    """
    Configuration for the EMA cross TWAP test strategy.
    """

    def __new__(cls, *args, **kwargs):
        kwargs.pop("exec_algorithm_id", None)
        kwargs.pop("twap_horizon_secs", None)
        kwargs.pop("twap_interval_secs", None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: str,
        bar_type: str,
        trade_size: str,
        fast_ema_period: int = 10,
        slow_ema_period: int = 20,
        exec_algorithm_id: str = "TWAP",
        twap_horizon_secs: float = 30.0,
        twap_interval_secs: float = 3.0,
        **kwargs,
    ):
        super().__init__(
            instrument_id=instrument_id,
            bar_type=bar_type,
            trade_size=trade_size,
            fast_ema_period=fast_ema_period,
            slow_ema_period=slow_ema_period,
            **kwargs,
        )
        self.exec_algorithm_id = exec_algorithm_id
        self.twap_horizon_secs = twap_horizon_secs
        self.twap_interval_secs = twap_interval_secs


class EMACrossTWAP(EMACross):
    """
    EMA cross test strategy submitting entries via the TWAP execution algorithm.
    """

    def __init__(self, config: EMACrossTWAPConfig):
        super().__init__(config)
        self._exec_algorithm_id = ExecAlgorithmId(config.exec_algorithm_id)
        self._exec_algorithm_params = {
            "horizon_secs": str(config.twap_horizon_secs),
            "interval_secs": str(config.twap_interval_secs),
        }

    def _submit_market(self, side: OrderSide):
        self._order_count += 1
        order = MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=self._instrument_id,
            client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
            order_side=side,
            quantity=self._trade_size,
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            reduce_only=False,
            quote_quantity=False,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            exec_algorithm_id=self._exec_algorithm_id,
            exec_algorithm_params=self._exec_algorithm_params,
        )
        self.submit_order(order)

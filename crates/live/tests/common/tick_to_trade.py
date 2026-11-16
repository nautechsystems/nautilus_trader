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
Python strategy source executed by the Rust tick-to-trade benchmark.
"""

from __future__ import annotations

from typing import TYPE_CHECKING


if TYPE_CHECKING:
    from nautilus_trader.model import ClientId
    from nautilus_trader.model import ClientOrderId
    from nautilus_trader.model import InstrumentId
    from nautilus_trader.model import OrderSide
    from nautilus_trader.model import Price
    from nautilus_trader.model import Quantity
    from nautilus_trader.model import QuoteTick
    from nautilus_trader.model import TradeTick
    from nautilus_trader.trading import Strategy
    from nautilus_trader.trading import StrategyConfig

# Executed as source by Rust, outside the Python package
# ruff: noqa: INP001


def _strategy_type(
    base: type[Strategy],
    instrument_id: InstrumentId,
    quote: bool,  # noqa: FBT001 - Rust supplies this positional argument
    quantity: Quantity,
    execution: tuple[ClientId, ClientId, OrderSide, type[ClientOrderId]],
) -> type[Strategy]:
    data_client, exec_client, buy, order_id = execution

    class TickStrategy(base):
        def __init__(self, config: StrategyConfig) -> None:
            super().__init__(config)
            self.count = 0

        def on_start(self) -> None:
            if quote:
                self.subscribe_quotes(instrument_id, client_id=data_client)
            else:
                self.subscribe_trades(instrument_id, client_id=data_client)

        def on_quote(self, tick: QuoteTick) -> None:
            self._submit(tick.bid_price)

        def on_trade(self, tick: TradeTick) -> None:
            self._submit(tick.price)

        def _submit(self, price: Price) -> None:
            order = self.order_factory.limit(
                instrument_id=instrument_id,
                order_side=buy,
                quantity=quantity,
                price=price,
                client_order_id=order_id(f"O-TTT-{self.count}"),
            )
            self.count += 1
            self.submit_order(order, client_id=exec_client)

    return TickStrategy

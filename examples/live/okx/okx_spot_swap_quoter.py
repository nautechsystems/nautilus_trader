#!/usr/bin/env python3
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
Run a quoter on both an OKX spot instrument and its perpetual swap.

WARNING: This example connects to the OKX live environment and places REAL orders
with REAL funds. On start it buys a small spot position, then maintains post-only
bid and ask quotes on both instruments. On stop it cancels all orders and closes
all positions. Spot orders use cash trade mode, so the spot side can only hold
long positions. The strategy has no alpha advantage whatsoever and is not
intended for production trading.

"""

from __future__ import annotations

from decimal import Decimal
from typing import Any
from typing import Self

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXDataClientFactory
from nautilus_trader.adapters.okx import OKXEnvironment
from nautilus_trader.adapters.okx import OKXExecutionClientConfig
from nautilus_trader.adapters.okx import OKXExecutionClientFactory
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.adapters.okx import OKXMarginMode
from nautilus_trader.common import Environment
from nautilus_trader.common import LogColor
from nautilus_trader.config import LiveExecutionEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.trading import Strategy


OKX_ENVIRONMENT = OKXEnvironment.LIVE
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("OKX-001")
STRATEGY_ID = StrategyId.from_str("OKX-SPOT-SWAP-QUOTER-001")

TOKEN = "ETH"
SPOT_INSTRUMENT_ID = InstrumentId.from_str(f"{TOKEN}-USDT.{OKX}")
SWAP_INSTRUMENT_ID = InstrumentId.from_str(f"{TOKEN}-USDT-SWAP.{OKX}")
INSTRUMENT_TYPES = [OKXInstrumentType.SPOT, OKXInstrumentType.SWAP]
INSTRUMENT_FAMILIES = ["ETH-USDT"]

SPOT_ORDER_QTY = Decimal("2.00")  # In quote currency (USDT)
SWAP_ORDER_QTY = Decimal("0.01")  # In contracts
TOB_OFFSET_TICKS = 100


class SpotSwapQuoterConfig(StrategyConfig):
    """
    Configuration for the spot and swap quoter strategy.
    """

    _CUSTOM_FIELDS = (
        "spot_instrument_id",
        "swap_instrument_id",
        "spot_order_qty",
        "swap_order_qty",
        "tob_offset_ticks",
        "log_data",
        "close_positions_on_stop",
    )

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        spot_instrument_id: InstrumentId,
        swap_instrument_id: InstrumentId,
        spot_order_qty: Decimal,
        swap_order_qty: Decimal,
        tob_offset_ticks: int = 100,
        log_data: bool = False,
        close_positions_on_stop: bool = True,
        **kwargs: Any,
    ) -> None:
        super().__init__()
        self.spot_instrument_id = spot_instrument_id
        self.swap_instrument_id = swap_instrument_id
        self.spot_order_qty = spot_order_qty
        self.swap_order_qty = swap_order_qty
        self.tob_offset_ticks = tob_offset_ticks
        self.log_data = log_data
        self.close_positions_on_stop = close_positions_on_stop


class SpotSwapQuoter(Strategy):
    """
    A quoter that places orders on both spot and swap instruments.

    Opens a position on start and maintains quotes on both instruments.

    """

    def __init__(self, config: SpotSwapQuoterConfig) -> None:
        super().__init__(config)
        self._config = config
        self.spot_instrument: Any | None = None
        self.swap_instrument: Any | None = None

        # Spot state
        self._spot_price_offset = Decimal(0)
        self._spot_order_qty: Quantity | None = None
        self._spot_bid_order: Any | None = None
        self._spot_ask_order: Any | None = None

        # Swap state
        self._swap_price_offset = Decimal(0)
        self._swap_order_qty: Quantity | None = None
        self._swap_bid_order: Any | None = None
        self._swap_ask_order: Any | None = None

    def on_start(self) -> None:
        self.spot_instrument = self.cache.instrument(self._config.spot_instrument_id)
        if self.spot_instrument is None:
            self.log.error(
                f"Could not find spot instrument for {self._config.spot_instrument_id}",
            )
            self.stop()
            return

        self.swap_instrument = self.cache.instrument(self._config.swap_instrument_id)
        if self.swap_instrument is None:
            self.log.error(f"Could not find swap instrument for {self._config.swap_instrument_id}")
            self.stop()
            return

        offset_ticks = max(self._config.tob_offset_ticks, 0)

        # Initialize spot parameters
        self._spot_price_offset = self.spot_instrument.price_increment.as_decimal() * offset_ticks
        self._spot_order_qty = Quantity.from_decimal_dp(
            self._config.spot_order_qty,
            self.spot_instrument.size_precision,
        )

        # Initialize swap parameters
        self._swap_price_offset = self.swap_instrument.price_increment.as_decimal() * offset_ticks
        self._swap_order_qty = Quantity.from_decimal_dp(
            self._config.swap_order_qty,
            self.swap_instrument.size_precision,
        )

        # Subscribe to quotes
        self.subscribe_quotes(self._config.spot_instrument_id)
        self.subscribe_quotes(self._config.swap_instrument_id)

        # Open initial position on spot
        self.open_position_on_start()

    def open_position_on_start(self) -> None:
        """
        Open a position on the spot instrument.
        """
        if self.spot_instrument is None or self._spot_order_qty is None:
            return

        order = self.order_factory.market(
            instrument_id=self._config.spot_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self._spot_order_qty,
            time_in_force=TimeInForce.GTC,
            quote_quantity=True,  # Market BUY orders use quote quantity (USDT)
        )

        self.submit_order(order)
        self.log.info(
            f"Opened position on {self._config.spot_instrument_id} with order {order.client_order_id}",
            LogColor.BLUE,
        )

    def on_quote(self, quote: QuoteTick) -> None:
        if self._config.log_data:
            self.log.info(repr(quote), LogColor.CYAN)

        if quote.instrument_id == self._config.spot_instrument_id:
            self._maintain_spot_orders(quote)
        elif quote.instrument_id == self._config.swap_instrument_id:
            self._maintain_swap_orders(quote)

    def _maintain_spot_orders(self, quote: QuoteTick) -> None:
        if self.spot_instrument is None or self._spot_order_qty is None:
            return

        if not self.is_running():
            # Don't create new orders if stopping
            return

        # Clear order references on any terminal status
        if self._spot_bid_order and self._spot_bid_order.is_closed():
            self._spot_bid_order = None
        if self._spot_ask_order and self._spot_ask_order.is_closed():
            self._spot_ask_order = None

        # Calculate desired prices with bounds checking
        desired_bid = quote.bid_price.as_decimal() - self._spot_price_offset
        desired_ask = quote.ask_price.as_decimal() + self._spot_price_offset

        # Guard against non-positive prices
        min_price = self.spot_instrument.price_increment.as_decimal()

        if desired_bid <= 0:
            self.log.warning(
                f"Calculated bid price {desired_bid} <= 0, using min price {min_price}",
            )
            desired_bid = min_price
        if desired_ask <= desired_bid:
            self.log.warning(f"Calculated ask price {desired_ask} <= bid {desired_bid}, skipping")
            return

        # Place BID order if none exists
        if self._spot_bid_order is None:
            price = self.spot_instrument.make_price(float(desired_bid))
            base_qty = self._spot_order_qty.as_decimal() / desired_bid
            quantity = Quantity.from_decimal_dp(base_qty, self.spot_instrument.size_precision)
            order = self.order_factory.limit(
                instrument_id=self._config.spot_instrument_id,
                order_side=OrderSide.BUY,
                quantity=quantity,
                price=price,
                post_only=True,
                quote_quantity=False,
            )
            self._spot_bid_order = order
            self.submit_order(order)

        # Place ASK order if none exists
        if self._spot_ask_order is None:
            price = self.spot_instrument.make_price(float(desired_ask))
            base_qty = self._spot_order_qty.as_decimal() / desired_ask
            quantity = Quantity.from_decimal_dp(base_qty, self.spot_instrument.size_precision)
            order = self.order_factory.limit(
                instrument_id=self._config.spot_instrument_id,
                order_side=OrderSide.SELL,
                quantity=quantity,
                price=price,
                post_only=True,
                quote_quantity=False,
            )
            self._spot_ask_order = order
            self.submit_order(order)

    def _maintain_swap_orders(self, quote: QuoteTick) -> None:
        if self.swap_instrument is None or self._swap_order_qty is None:
            return

        if not self.is_running():
            # Don't create new orders if stopping
            return

        # Clear order references on any terminal status
        if self._swap_bid_order and self._swap_bid_order.is_closed():
            self._swap_bid_order = None
        if self._swap_ask_order and self._swap_ask_order.is_closed():
            self._swap_ask_order = None

        # Calculate desired prices with bounds checking
        desired_bid = quote.bid_price.as_decimal() - self._swap_price_offset
        desired_ask = quote.ask_price.as_decimal() + self._swap_price_offset

        # Guard against non-positive prices
        min_price = self.swap_instrument.price_increment.as_decimal()

        if desired_bid <= 0:
            self.log.warning(
                f"Calculated swap bid price {desired_bid} <= 0, using min price {min_price}",
            )
            desired_bid = min_price
        if desired_ask <= desired_bid:
            self.log.warning(
                f"Calculated swap ask price {desired_ask} <= bid {desired_bid}, skipping",
            )
            return

        # Place BID order if none exists
        if self._swap_bid_order is None:
            price = self.swap_instrument.make_price(float(desired_bid))
            order = self.order_factory.limit(
                instrument_id=self._config.swap_instrument_id,
                order_side=OrderSide.BUY,
                quantity=self._swap_order_qty,
                price=price,
                post_only=True,
                quote_quantity=False,
            )
            self._swap_bid_order = order
            self.submit_order(order)

        # Place ASK order if none exists
        if self._swap_ask_order is None:
            price = self.swap_instrument.make_price(float(desired_ask))
            order = self.order_factory.limit(
                instrument_id=self._config.swap_instrument_id,
                order_side=OrderSide.SELL,
                quantity=self._swap_order_qty,
                price=price,
                post_only=True,
                quote_quantity=False,
            )
            self._swap_ask_order = order
            self.submit_order(order)

    def on_order_filled(self, event: OrderFilled) -> None:
        # Reset state on fills so quotes are re-placed
        if self._spot_bid_order and event.client_order_id == self._spot_bid_order.client_order_id:
            self._spot_bid_order = None
        elif self._spot_ask_order and event.client_order_id == self._spot_ask_order.client_order_id:
            self._spot_ask_order = None

        if self._swap_bid_order and event.client_order_id == self._swap_bid_order.client_order_id:
            self._swap_bid_order = None
        elif self._swap_ask_order and event.client_order_id == self._swap_ask_order.client_order_id:
            self._swap_ask_order = None

    def on_stop(self) -> None:
        self.cancel_all_orders(self._config.spot_instrument_id)
        self.cancel_all_orders(self._config.swap_instrument_id)

        if self._config.close_positions_on_stop:
            self.close_all_positions(self._config.spot_instrument_id)
            self.close_all_positions(self._config.swap_instrument_id)

        # Reset state
        self._spot_bid_order = None
        self._spot_ask_order = None
        self._swap_bid_order = None
        self._swap_ask_order = None


def main() -> None:
    node = (
        LiveNode.builder("OKX-SPOT-SWAP-QUOTER-001", TRADER_ID, Environment.LIVE)
        .with_exec_engine_config(
            LiveExecutionEngineConfig(
                reconciliation_instrument_ids=[
                    str(SPOT_INSTRUMENT_ID),
                    str(SWAP_INSTRUMENT_ID),
                ],
            ),
        )
        .with_reconciliation(True)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))  # Must bypass for spot for now
        .with_timeout_connection(20)
        .with_timeout_reconciliation(10)
        .with_timeout_portfolio(10)
        .with_timeout_disconnection_secs(10)
        .with_delay_post_stop_secs(5)
        .add_data_client(
            None,
            OKXDataClientFactory(),
            OKXDataClientConfig(
                instrument_types=INSTRUMENT_TYPES,
                instrument_families=INSTRUMENT_FAMILIES,
                environment=OKX_ENVIRONMENT,
            ),
        )
        .add_exec_client(
            None,
            OKXExecutionClientFactory(),
            OKXExecutionClientConfig(
                account_id=ACCOUNT_ID,
                instrument_types=INSTRUMENT_TYPES,
                environment=OKX_ENVIRONMENT,
                margin_mode=OKXMarginMode.CROSS,
            ),
        )
        .build()
    )
    node.add_strategy(
        SpotSwapQuoter(
            SpotSwapQuoterConfig(
                spot_instrument_id=SPOT_INSTRUMENT_ID,
                swap_instrument_id=SWAP_INSTRUMENT_ID,
                spot_order_qty=SPOT_ORDER_QTY,
                swap_order_qty=SWAP_ORDER_QTY,
                tob_offset_ticks=TOB_OFFSET_TICKS,
                strategy_id=STRATEGY_ID,
                use_hyphens_in_client_order_ids=False,  # OKX doesn't allow hyphens
            ),
        ),
    )

    node.run()


if __name__ == "__main__":
    main()

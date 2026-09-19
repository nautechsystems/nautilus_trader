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
Submit a single resting limit order on Hyperliquid testnet.

This example connects to Hyperliquid TESTNET and submits exactly one post-only
limit order on the first quote received, priced `tob_offset_ticks` away from the
touch so it rests rather than crossing. On stop it cancels the order. It never
opens or closes positions, and it never resubmits.

Requires `HYPERLIQUID_TESTNET_PK` to be set. If that key is an agent (API)
wallet, `HYPERLIQUID_ACCOUNT_ADDRESS` must also be set to the master account
address, otherwise order status reports and user feeds come back empty.

Hyperliquid enforces a minimum order notional (perps $10, spot 10 USDC), so size
`ORDER_QTY` such that `ORDER_QTY * limit_price` clears it. The limit price, not the
market price, is what counts: a large `TOB_OFFSET_TICKS` lowers the notional of a buy.

"""

from __future__ import annotations

from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidEnvironment
from nautilus_trader.adapters.hyperliquid import HyperliquidExecutionClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.common import LogColor
from nautilus_trader.config import LiveExecutionEngineConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderRejected
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.trading import Strategy


# WARNING: With DRY_RUN = False, this example submits a real order to Hyperliquid
# testnet. Set DRY_RUN = True to connect and log the intended order without
# submitting it.
DRY_RUN = False
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("HYPERLIQUID-001")
STRATEGY_ID = StrategyId.from_str("HYPERLIQUID-LIMIT-ORDER-001")
INSTRUMENT_ID = InstrumentId.from_str(f"ETH-USD-PERP.{HYPERLIQUID}")
ORDER_SIDE = OrderSide.BUY
ORDER_QTY = Decimal("0.01")
# Distance from the touch, in price increments, so the order rests rather than fills.
# This is instrument-specific: ETH-USD-PERP has a 0.01 price increment, so 20,000 ticks
# is $200 (roughly 8% away at current testnet prices). Recheck it for other instruments.
TOB_OFFSET_TICKS = 20_000


class LimitOrderConfig(StrategyConfig):
    """
    Configuration for the single limit order strategy.
    """

    def __init__(
        self,
        *,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        order_qty: Decimal,
        tob_offset_ticks: int = 20_000,
        post_only: bool = True,
        cancel_on_stop: bool = True,
        dry_run: bool = False,
        **_kwargs: Any,
    ) -> None:
        """
        Initialize the configuration.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.order_side = order_side
        self.order_qty = order_qty
        self.tob_offset_ticks = tob_offset_ticks
        self.post_only = post_only
        self.cancel_on_stop = cancel_on_stop
        self.dry_run = dry_run


class LimitOrderStrategy(Strategy):
    """
    Submits a single resting limit order on the first quote received.

    The order is priced away from the touch by `tob_offset_ticks` so it rests on
    the book. Nothing is resubmitted after the first submission.

    """

    def __init__(self, config: LimitOrderConfig) -> None:
        """
        Initialize the strategy.
        """
        super().__init__(config)
        self._config = config
        self.instrument: Any | None = None
        self._price_offset = Decimal(0)
        self._order_qty: Quantity | None = None
        self._submitted = False

    def on_start(self) -> None:
        """
        On start.
        """
        self.instrument = self.cache.instrument(self._config.instrument_id)
        if self.instrument is None:
            log_msg = f"Could not find instrument for {self._config.instrument_id}"
            self.log.error(log_msg)
            self.stop()
            return

        offset_ticks = max(self._config.tob_offset_ticks, 0)
        self._price_offset = self.instrument.price_increment.as_decimal() * offset_ticks
        self._order_qty = Quantity.from_decimal_dp(
            self._config.order_qty,
            self.instrument.size_precision,
        )

        self.subscribe_quotes(self._config.instrument_id)

    def on_quote(self, quote: QuoteTick) -> None:
        """
        On quote.
        """
        if self._submitted or self.instrument is None or self._order_qty is None:
            return

        if not self.is_running():
            return

        if self._config.order_side == OrderSide.BUY:
            desired_price = quote.bid_price.as_decimal() - self._price_offset
        else:
            desired_price = quote.ask_price.as_decimal() + self._price_offset

        min_price = self.instrument.price_increment.as_decimal()
        if desired_price < min_price:
            log_msg = (
                f"Calculated price {desired_price} below the minimum increment {min_price}, "
                "reduce `tob_offset_ticks`"
            )
            self.log.warning(log_msg)
            return

        price = self.instrument.make_price(float(desired_price))
        order = self.order_factory.limit(
            instrument_id=self._config.instrument_id,
            order_side=self._config.order_side,
            quantity=self._order_qty,
            price=price,
            time_in_force=TimeInForce.GTC,  # Hyperliquid supports GTC and IOC only
            post_only=self._config.post_only,
        )

        self._submitted = True  # Submit at most once

        if self._config.dry_run:
            log_msg = f"[DRY_RUN] Would submit {order!r}"
            self.log.info(log_msg, LogColor.YELLOW)
            return

        self.submit_order(order)
        log_msg = f"Submitted {self._config.order_side!r} limit for {self._order_qty} @ {price}"
        self.log.info(log_msg, LogColor.BLUE)

    def on_order_accepted(self, event: OrderAccepted) -> None:
        """
        On order accepted.
        """
        log_msg = f"Order resting: {event.client_order_id}"
        self.log.info(log_msg, LogColor.GREEN)

    def on_order_rejected(self, event: OrderRejected) -> None:
        """
        On order rejected.
        """
        log_msg = f"Order rejected: {event.client_order_id} {event.reason}"
        self.log.error(log_msg)

    def on_order_filled(self, event: OrderFilled) -> None:
        """
        On order filled.
        """
        log_msg = f"Order filled: {event.client_order_id} {event.last_qty} @ {event.last_px}"
        self.log.info(log_msg, LogColor.MAGENTA)

    def on_order_canceled(self, event: OrderCanceled) -> None:
        """
        On order canceled.
        """
        log_msg = f"Order canceled: {event.client_order_id}"
        self.log.info(log_msg, LogColor.CYAN)

    def on_stop(self) -> None:
        """
        On stop.
        """
        if self._config.cancel_on_stop and not self._config.dry_run:
            self.cancel_all_orders(self._config.instrument_id)


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("HYPERLIQUID-LIMIT-ORDER-001", TRADER_ID, Environment.LIVE)
        .with_exec_engine_config(
            LiveExecutionEngineConfig(
                reconciliation_instrument_ids=[str(INSTRUMENT_ID)],
            ),
        )
        .with_reconciliation(reconciliation=True)
        .add_data_client(
            None,
            HyperliquidDataClientFactory(),
            HyperliquidDataClientConfig(environment=HyperliquidEnvironment.TESTNET),
        )
        .add_exec_client(
            None,
            HyperliquidExecutionClientFactory(),
            HyperliquidExecutionClientConfig(
                account_id=ACCOUNT_ID,
                environment=HyperliquidEnvironment.TESTNET,
            ),
        )
        .build()
    )
    node.add_strategy(
        LimitOrderStrategy(
            LimitOrderConfig(
                instrument_id=INSTRUMENT_ID,
                order_side=ORDER_SIDE,
                order_qty=ORDER_QTY,
                tob_offset_ticks=TOB_OFFSET_TICKS,
                post_only=True,
                cancel_on_stop=True,
                dry_run=DRY_RUN,
                strategy_id=STRATEGY_ID,
                external_order_instrument_ids=[INSTRUMENT_ID],
            ),
        ),
    )

    node.run()


if __name__ == "__main__":
    main()

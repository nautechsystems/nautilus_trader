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
Hyperliquid priceBucket (Up/Range/Down) paper-trading smoke test.

This example targets recurring `priceBucket` questions (commonly `period:15m`) and demonstrates:
- Selecting the active named outcome instrument by `underlying/period/bucket_index`
- Subscribing to quotes for that instrument
- Submitting a sandbox market order (paper trading)

Environment
-----------
- Uses Hyperliquid TESTNET by default.
- No Hyperliquid private key is required (data-only + sandbox execution).

"""

from __future__ import annotations

import os
from datetime import timedelta
from decimal import Decimal

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.paper import get_price_bucket_thresholds
from nautilus_trader.adapters.hyperliquid.paper import select_active_price_bucket_instrument
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


class PriceBucketSmokeConfig(StrategyConfig, frozen=True):
    underlying: str = "BTC"
    period: str = "15m"
    bucket_index: int = 0
    client_id: ClientId = ClientId("SANDBOX")
    order_qty: Decimal = Decimal(100)
    limit_price: float = 0.5


class PriceBucketSmoke(Strategy):
    def __init__(self, config: PriceBucketSmokeConfig):
        PyCondition.type(config, PriceBucketSmokeConfig, "config")
        super().__init__(config)

        self._instrument_id: InstrumentId | None = None
        self._order_submitted: bool = False
        self._initialized: bool = False

    def on_start(self) -> None:
        # Instruments are loaded asynchronously by the data client; delay selection until the cache
        # is populated.
        self.clock.set_timer(
            name="INIT_PRICE_BUCKET",
            interval=timedelta(seconds=1),
            callback=self._try_initialize,
        )

    def _try_initialize(self, _time_event) -> None:
        if self._initialized:
            return

        instruments = self.cache.instruments()
        if not instruments:
            return

        selected = select_active_price_bucket_instrument(
            instruments,
            underlying=self.config.underlying,
            period=self.config.period,
            bucket_index=self.config.bucket_index,
            side="YES",
        )
        self._instrument_id = selected
        self._initialized = True

        instrument = self.cache.instrument(selected)
        if instrument is None:
            self.log.error(f"Instrument not found in cache: {selected}")
            self.stop()
            return

        low, high = get_price_bucket_thresholds(instrument)
        self.log.info(
            f"Selected priceBucket instrument: {selected} "
            f"(bucket_index={self.config.bucket_index}, thresholds={low},{high})",
            color=LogColor.CYAN,
        )

        self.subscribe_quote_ticks(selected)

        # Submit an out-of-the-box limit order after a short delay so the paper
        # trading path is exercised even if this testnet instrument has no live
        # quotes during the run window.
        self.clock.set_timer(
            name="SUBMIT_LIMIT",
            interval=timedelta(seconds=2),
            callback=self._submit_limit_order,
        )

    def _submit_limit_order(self, _time_event) -> None:
        if self._order_submitted or self._instrument_id is None:
            return

        instrument = self.cache.instrument(self._instrument_id)
        if instrument is None:
            return

        qty = Quantity(self.config.order_qty, instrument.size_precision)
        order = self.order_factory.limit(
            instrument_id=self._instrument_id,
            order_side=OrderSide.BUY,
            quantity=qty,
            price=Price(self.config.limit_price, instrument.price_precision),
            time_in_force=TimeInForce.GTC,
            post_only=True,
        )
        self.submit_order(order, client_id=self.config.client_id)
        self._order_submitted = True
        self.log.info(
            f"Submitted sandbox BUY limit order: {self._instrument_id} @ {self.config.limit_price}",
            LogColor.GREEN,
        )


def main() -> None:
    environment_str = os.getenv("HYPERLIQUID_ENV", "TESTNET").strip().upper()
    environment = (
        HyperliquidEnvironment.MAINNET
        if environment_str == "MAINNET"
        else HyperliquidEnvironment.TESTNET
    )

    config_node = TradingNodeConfig(
        trader_id=TraderId("OUTCOME-BUCKET-SMOKE-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_colors=True,
            use_pyo3=True,
        ),
        data_clients={
            HYPERLIQUID: HyperliquidDataClientConfig(
                environment=environment,
                instrument_provider=InstrumentProviderConfig(load_all=True),
                product_types=(HyperliquidProductType.OUTCOME,),
                update_outcome_instruments_on_expiry=True,
            ),
        },
        exec_clients={
            "SANDBOX": SandboxExecutionClientConfig(
                venue=HYPERLIQUID,
                base_currency="USDH",
                starting_balances=["10_000 USDH"],
                account_type="MARGIN",
                oms_type="NETTING",
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    node = TradingNode(config=config_node)
    node.trader.add_strategy(PriceBucketSmoke(config=PriceBucketSmokeConfig(strategy_id="SMOKE")))
    node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
    node.add_exec_client_factory("SANDBOX", SandboxLiveExecClientFactory)

    node.build()

    try:
        node.run(raise_exception=True)
    finally:
        node.dispose()


if __name__ == "__main__":
    main()

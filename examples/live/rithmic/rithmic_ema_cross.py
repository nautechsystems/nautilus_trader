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
Example: run the built-in EMACross strategy live on Rithmic bars.

This example:
1. Resolves the current front-month contract for a Rithmic product root
2. Builds a standard Nautilus live node with both data and execution clients
3. Subscribes to live 15-second Rithmic bars by default
4. Runs the existing EMACross strategy against the resolved futures contract

Required environment variables:
    RITHMIC_USERNAME
    RITHMIC_PASSWORD
    RITHMIC_SYSTEM_NAME
    RITHMIC_ACCOUNT_ID

Optional environment variables:
    RITHMIC_PROFILE
    RITHMIC_ENV
    RITHMIC_FCM_ID
    RITHMIC_IB_ID
    RITHMIC_APP_NAME
    RITHMIC_APP_VERSION

Notes on app credentials:
    If your credentials were not issued with app details, a temporary working
    fallback is already available, so users do not need to run Rithmic
    conformance themselves unless instructed otherwise.
    For direct API onboarding later: https://www.rithmic.com/api-request

Example configuration:
    Edit the constants below for product, exchange, bar type, EMA periods,
    trade size, warmup window, and optional auto-stop duration.

Warning:
    This example can submit live orders to the configured account.
    Use a demo account first.

Notes:
    By default, this example warms the EMAs from native Rithmic historical bars
    before live execution begins.
    Internal bars remain available, but historical warmup is only requested when
    using externally aggregated Rithmic bars.
"""

from __future__ import annotations

import asyncio
import os
import threading
from datetime import timedelta
from decimal import Decimal
from typing import TYPE_CHECKING

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic import RithmicLiveExecClientFactory
from nautilus_trader.adapters.rithmic.bindings import RithmicGateway
from nautilus_trader.adapters.rithmic.bindings import (
    RithmicInstrumentProvider as BindingInstrumentProvider,
)
from nautilus_trader.adapters.rithmic.config import to_binding_environment
from nautilus_trader.adapters.rithmic.providers import (
    RithmicInstrumentProvider as AdapterInstrumentProvider,
)
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


if TYPE_CHECKING:
    from nautilus_trader.model.instruments import Instrument


# Edit these example settings directly instead of exporting per-run strategy env vars.
PRODUCT = "MNQ"
EXCHANGE = "CME"
BAR_SPEC = "15-SECOND-LAST-EXTERNAL"
TRADE_SIZE = 1
FAST_EMA_PERIOD = 10
SLOW_EMA_PERIOD = 20
WARMUP_MINUTES = 30
RUN_SECONDS = 0


def build_gateway(config: RithmicDataClientConfig) -> RithmicGateway:
    return RithmicGateway(
        environment=to_binding_environment(config.environment),
        username=config.username,
        password=config.password,
        system_name=config.system_name,
        app_name=config.app_name,
        app_version=config.app_version,
        fcm_id=config.fcm_id or "",
        ib_id=config.ib_id or "",
        account_id="",
        enable_ticker=True,
        enable_order=False,
        enable_pnl=False,
        enable_history=False,
    )


async def resolve_front_month_instrument(
    profile: str | None,
    product: str,
    exchange: str,
) -> tuple[Instrument, str]:
    config = RithmicDataClientConfig.from_env(profile)
    gateway = build_gateway(config)
    provider = BindingInstrumentProvider(gateway)
    converter = AdapterInstrumentProvider(config)

    await gateway.connect()
    try:
        contract = await provider.load_front_month_async(product, exchange)
    finally:
        await gateway.disconnect()

    resolved_exchange = getattr(contract, "exchange", None) or exchange
    return converter._convert_instrument(contract), resolved_exchange


def build_provider_config(
    instrument_id: InstrumentId,
    exchange: str,
) -> InstrumentProviderConfig:
    return InstrumentProviderConfig(
        load_all=False,
        load_ids=frozenset([instrument_id]),
        filters={"exchange": exchange},
    )


def build_data_client_config(
    profile: str | None,
    instrument_id: InstrumentId,
    exchange: str,
    enable_history: bool,
) -> RithmicDataClientConfig:
    base = RithmicDataClientConfig.from_env(profile)
    return RithmicDataClientConfig(
        environment=base.environment,
        username=base.username,
        password=base.password,
        system_name=base.system_name,
        app_name=base.app_name,
        app_version=base.app_version,
        fcm_id=base.fcm_id,
        ib_id=base.ib_id,
        enable_history=enable_history,
        instrument_provider=build_provider_config(instrument_id, exchange),
    )


def build_exec_client_config(
    profile: str | None,
    instrument_id: InstrumentId,
    exchange: str,
) -> RithmicExecClientConfig:
    base = RithmicExecClientConfig.from_env(profile)
    return RithmicExecClientConfig(
        environment=base.environment,
        username=base.username,
        password=base.password,
        system_name=base.system_name,
        account_id=base.account_id,
        app_name=base.app_name,
        app_version=base.app_version,
        fcm_id=base.fcm_id,
        ib_id=base.ib_id,
        execution_replay_lookback_secs=base.execution_replay_lookback_secs,
        native_bracket_state_path=base.native_bracket_state_path,
        instrument_provider=build_provider_config(instrument_id, exchange),
    )


class LiveRithmicEMACross(EMACross):
    def __init__(self, config: EMACrossConfig, warmup_minutes: int, exchange: str) -> None:
        super().__init__(config)
        self._exchange = exchange
        self._warmup_minutes = warmup_minutes
        self._started = False

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.warning(
                f"Instrument {self.config.instrument_id} not yet cached; requesting definition",
            )
            self.request_instrument(
                self.config.instrument_id,
                params={"exchange": self._exchange},
            )
            return

        self._start_strategy()

    def on_instrument(self, instrument) -> None:
        if instrument.id != self.config.instrument_id or self._started:
            return

        self.instrument = instrument
        self._start_strategy()

    def _start_strategy(self) -> None:
        if self._started or self.instrument is None:
            return

        self._started = True

        self.register_indicator_for_bars(self.config.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.config.bar_type, self.slow_ema)

        if self.config.request_bars and self._warmup_minutes > 0:
            self.request_bars(
                self.config.bar_type,
                start=self._clock.utc_now() - timedelta(minutes=self._warmup_minutes),
            )

        self.subscribe_bars(self.config.bar_type)

        if self.config.subscribe_quote_ticks:
            self.subscribe_quote_ticks(self.config.instrument_id)
        if self.config.subscribe_trade_ticks:
            self.subscribe_trade_ticks(self.config.instrument_id)


def schedule_stop(node: TradingNode, run_seconds: int) -> threading.Timer | None:
    if run_seconds <= 0:
        return None

    loop = node.get_event_loop()
    if loop is None:
        raise RuntimeError("Trading node has no event loop")

    def stop_node() -> None:
        loop.call_soon_threadsafe(node.stop)

    timer = threading.Timer(run_seconds, stop_node)
    timer.daemon = True
    timer.start()
    return timer


def main() -> None:
    profile = os.environ.get("RITHMIC_PROFILE")
    product = PRODUCT.strip().upper()
    exchange = EXCHANGE.strip().upper()
    bar_spec = BAR_SPEC.strip().upper()
    trade_size = Decimal(str(TRADE_SIZE))
    fast_period = int(FAST_EMA_PERIOD)
    slow_period = int(SLOW_EMA_PERIOD)
    warmup_minutes = int(WARMUP_MINUTES)
    run_seconds = int(RUN_SECONDS)

    if trade_size <= 0:
        raise ValueError("TRADE_SIZE must be positive")
    if warmup_minutes < 0:
        raise ValueError("WARMUP_MINUTES cannot be negative")
    if run_seconds < 0:
        raise ValueError("RUN_SECONDS cannot be negative")

    instrument, instrument_exchange = asyncio.run(
        resolve_front_month_instrument(profile, product, exchange),
    )
    instrument_id = instrument.id
    bar_type = BarType.from_str(f"{instrument_id}-{bar_spec}")
    use_history = bar_type.is_externally_aggregated()
    request_bars = use_history and warmup_minutes > 0

    data_config = build_data_client_config(
        profile,
        instrument_id,
        instrument_exchange,
        enable_history=use_history,
    )
    exec_config = build_exec_client_config(profile, instrument_id, instrument_exchange)

    config_node = TradingNodeConfig(
        trader_id=TraderId("TESTER-001"),
        logging=LoggingConfig(log_level="INFO", use_pyo3=True),
        exec_engine=LiveExecEngineConfig(
            reconciliation=True,
            reconciliation_instrument_ids=[instrument_id],
            open_check_interval_secs=5.0,
            open_check_open_only=False,
        ),
        data_clients={RITHMIC: data_config},
        exec_clients={RITHMIC: exec_config},
        timeout_connection=10.0,
        timeout_reconciliation=10.0,
        timeout_disconnection=5.0,
        timeout_post_stop=2.0,
        timeout_shutdown=2.0,
    )

    strategy = LiveRithmicEMACross(
        config=EMACrossConfig(
            instrument_id=instrument_id,
            external_order_claims=[instrument_id],
            bar_type=bar_type,
            trade_size=trade_size,
            fast_ema_period=fast_period,
            slow_ema_period=slow_period,
            subscribe_quote_ticks=False,
            subscribe_trade_ticks=False,
            request_bars=request_bars,
            order_id_tag="rithmic-ema",
        ),
        warmup_minutes=warmup_minutes,
        exchange=instrument_exchange,
    )

    node = TradingNode(config=config_node)
    node.cache.add_instrument(instrument)
    node.trader.add_strategy(strategy)
    node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
    node.add_exec_client_factory(RITHMIC, RithmicLiveExecClientFactory)
    node.build()

    print("Rithmic Live EMA Cross")
    print("=" * 50)
    print(f"Profile: {profile or '<default>'}")
    print(f"Requested root: {product}:{exchange}")
    print(f"Resolved instrument: {instrument_id}")
    print(f"Bar type: {bar_type}")
    print(f"Trade size: {trade_size}")
    print(f"Fast/slow EMA periods: {fast_period}/{slow_period}")
    if request_bars:
        print(f"Historical warmup: {warmup_minutes} minutes")
    else:
        print("Historical warmup: disabled")
    if run_seconds > 0:
        print(f"Auto-stop after: {run_seconds} seconds")
    else:
        print("Auto-stop after: disabled")
    print()
    print("WARNING: this example can submit live orders to the configured account.")
    print("Use a demo account first.")
    if request_bars:
        print("EMAs warm up from native Rithmic historical bars before live execution begins.")
    elif use_history:
        print("Live native Rithmic bars enabled with no historical warmup.")
    else:
        print("EMAs warm up from live internal bars only; no historical warmup is requested.")

    stop_timer = schedule_stop(node, run_seconds)

    try:
        node.run()
    except KeyboardInterrupt:
        node.stop()
    finally:
        if stop_timer is not None:
            stop_timer.cancel()
        node.dispose()


if __name__ == "__main__":
    main()

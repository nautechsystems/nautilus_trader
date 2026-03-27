# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.17.3
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# # Rithmic Live Strategy Sandbox
#
# Objective:
# - Resolve the current front month for a Rithmic product root.
# - Subscribe to live quote and trade feeds through the adapter.
# - Build internal bars inside Nautilus from the live tick stream.
# - Observe a simple strategy-style indicator pipeline without submitting orders.

# %% [markdown]
# Note: Use the jupytext python package to open this file as a notebook in Jupyter.
# Also run `jupytext-config set-default-viewer` if you want `.py` notebook files to open as notebooks by default.

# %% [markdown]
# ## Imports

# %%
from __future__ import annotations

import asyncio
import os
import threading

import pandas as pd

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic.bindings import RithmicGateway
from nautilus_trader.adapters.rithmic.bindings import (
    RithmicInstrumentProvider as BindingInstrumentProvider,
)
from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import to_binding_environment
from nautilus_trader.adapters.rithmic.factories import RithmicLiveDataClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy

try:
    asyncio.get_running_loop()
except RuntimeError:
    _RUNNING_LOOP = None
else:
    import nest_asyncio

    nest_asyncio.apply()
    _RUNNING_LOOP = asyncio.get_running_loop()


def run_async(coro):
    if _RUNNING_LOOP and _RUNNING_LOOP.is_running():
        return _RUNNING_LOOP.run_until_complete(coro)
    return asyncio.run(coro)


def in_ipython() -> bool:
    try:
        from IPython import get_ipython
    except ImportError:
        return False

    return get_ipython() is not None


SCRIPT_MODE = not in_ipython()


# %% [markdown]
# ## Parameters

# %%
PROFILE = os.environ.get("RITHMIC_PROFILE", "Apex")
PRODUCT = os.environ.get("RITHMIC_NOTEBOOK_ROOT", "MNQ")
EXCHANGE = os.environ.get("RITHMIC_NOTEBOOK_EXCHANGE", "CME")
LIVE_BAR_SPEC = os.environ.get("RITHMIC_NOTEBOOK_LIVE_BAR_SPEC", "1-MINUTE-LAST-INTERNAL")
RUN_SECONDS = int(os.environ.get("RITHMIC_NOTEBOOK_RUN_SECONDS", "75" if SCRIPT_MODE else "0"))
FAST_EMA_PERIOD = int(os.environ.get("RITHMIC_NOTEBOOK_FAST_EMA_PERIOD", "10"))
SLOW_EMA_PERIOD = int(os.environ.get("RITHMIC_NOTEBOOK_SLOW_EMA_PERIOD", "20"))
LOG_EVERY_N_TICKS = int(os.environ.get("RITHMIC_NOTEBOOK_LOG_EVERY_N_TICKS", "50"))
VERBOSE_TICK_LOGS = os.environ.get("RITHMIC_NOTEBOOK_VERBOSE_TICKS", "0") == "1"

{
    "profile": PROFILE,
    "product": PRODUCT,
    "exchange": EXCHANGE,
    "live_bar_spec": LIVE_BAR_SPEC,
    "run_seconds": RUN_SECONDS,
    "fast_ema_period": FAST_EMA_PERIOD,
    "slow_ema_period": SLOW_EMA_PERIOD,
}


# %% [markdown]
# ## Helpers

# %%
def build_data_client_config(
    profile: str,
    exchange: str,
    load_ids: frozenset[InstrumentId] | None = None,
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
        server=base.server,
        alt_server=base.alt_server,
        enable_history=False,
        instrument_provider=InstrumentProviderConfig(
            load_all=False,
            load_ids=load_ids,
            filters={"exchange": exchange},
        ),
    )


async def resolve_front_month_contract(
    config: RithmicDataClientConfig,
    product: str,
    exchange: str,
):
    gateway = RithmicGateway(
        environment=to_binding_environment(config.environment),
        username=config.username,
        password=config.password,
        system_name=config.system_name,
        app_name=config.app_name,
        app_version=config.app_version,
        fcm_id=config.fcm_id or "",
        ib_id=config.ib_id or "",
        account_id="",
        server=config.server,
        alt_server=config.alt_server,
        enable_ticker=True,
        enable_order=False,
        enable_pnl=False,
        enable_history=False,
    )
    provider = BindingInstrumentProvider(gateway)
    await gateway.connect()
    try:
        return await provider.load_front_month_async(product, exchange)
    finally:
        await gateway.disconnect()


async def load_nautilus_instrument(
    config: RithmicDataClientConfig,
    instrument_id: InstrumentId,
    exchange: str,
):
    from nautilus_trader.adapters.rithmic.providers import (
        RithmicInstrumentProvider as PythonInstrumentProvider,
    )

    gateway = RithmicGateway(
        environment=to_binding_environment(config.environment),
        username=config.username,
        password=config.password,
        system_name=config.system_name,
        app_name=config.app_name,
        app_version=config.app_version,
        fcm_id=config.fcm_id or "",
        ib_id=config.ib_id or "",
        account_id="",
        server=config.server,
        alt_server=config.alt_server,
        enable_ticker=True,
        enable_order=False,
        enable_pnl=False,
        enable_history=False,
    )
    provider = PythonInstrumentProvider(config)

    await gateway.connect()
    provider.bind_gateway(gateway)
    try:
        await provider.load_async(instrument_id, filters={"exchange": exchange})
        instrument = provider.find(instrument_id)
        if instrument is None:
            raise RuntimeError(f"Failed to load instrument {instrument_id}")
        return instrument
    finally:
        provider.clear_gateway_binding()
        await gateway.disconnect()


def resolve_nautilus_instrument(
    config: RithmicDataClientConfig,
    product: str,
    exchange: str,
):
    front_month = run_async(resolve_front_month_contract(config, product, exchange))
    instrument_id = InstrumentId.from_str(f"{front_month.symbol}.{exchange}.{RITHMIC}")
    instrument = run_async(load_nautilus_instrument(config, instrument_id, exchange))

    return front_month, instrument


def ts_to_utc(value: int) -> pd.Timestamp:
    return pd.Timestamp(value, unit="ns", tz="UTC")


def schedule_stop(node: TradingNode, run_seconds: int):
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


class LiveProbeConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    fast_ema_period: int = 10
    slow_ema_period: int = 20
    log_every_n_ticks: int = 50
    verbose_tick_logs: bool = False


class RithmicLiveProbe(Strategy):
    def __init__(self, config: LiveProbeConfig) -> None:
        super().__init__(config)
        self.instrument: Instrument | None = None
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)
        self.quote_count = 0
        self.trade_count = 0
        self.bar_count = 0
        self.last_quote: dict | None = None
        self.last_trade: dict | None = None
        self.bar_snapshots: list[dict] = []

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        self.register_indicator_for_bars(self.config.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.config.bar_type, self.slow_ema)

        self.subscribe_trade_ticks(self.config.instrument_id)
        self.subscribe_quote_ticks(self.config.instrument_id)
        self.subscribe_bars(self.config.bar_type)

        self.log.info(
            f"Subscribed to quotes, trades, and internal bars for {self.config.instrument_id}",
            LogColor.GREEN,
        )

    def on_quote_tick(self, tick: QuoteTick) -> None:
        self.quote_count += 1
        self.last_quote = {
            "ts_event": ts_to_utc(tick.ts_event),
            "bid": float(tick.bid_price),
            "ask": float(tick.ask_price),
            "bid_size": float(tick.bid_size),
            "ask_size": float(tick.ask_size),
        }

        if self.config.verbose_tick_logs or self.quote_count % max(self.config.log_every_n_ticks, 1) == 0:
            self.log.info(
                f"Quote[{self.quote_count}] bid={tick.bid_price} ask={tick.ask_price}",
                LogColor.CYAN,
            )

    def on_trade_tick(self, tick: TradeTick) -> None:
        self.trade_count += 1
        self.last_trade = {
            "ts_event": ts_to_utc(tick.ts_event),
            "price": float(tick.price),
            "size": float(tick.size),
            "aggressor_side": tick.aggressor_side.name,
        }

        if self.config.verbose_tick_logs or self.trade_count % max(self.config.log_every_n_ticks, 1) == 0:
            self.log.info(
                f"Trade[{self.trade_count}] price={tick.price} size={tick.size}",
                LogColor.MAGENTA,
            )

    def on_bar(self, bar: Bar) -> None:
        self.bar_count += 1
        snapshot = {
            "bar_index": self.bar_count,
            "ts_event": ts_to_utc(bar.ts_event),
            "open": float(bar.open),
            "high": float(bar.high),
            "low": float(bar.low),
            "close": float(bar.close),
            "volume": float(bar.volume),
            "quotes_seen": self.quote_count,
            "trades_seen": self.trade_count,
            "fast_ema": float(self.fast_ema.value),
            "slow_ema": float(self.slow_ema.value),
            "indicators_ready": self.indicators_initialized(),
        }
        self.bar_snapshots.append(snapshot)
        self.log.info(
            (
                f"InternalBar[{self.bar_count}] close={bar.close} volume={bar.volume} "
                f"fast_ema={self.fast_ema.value:.4f} slow_ema={self.slow_ema.value:.4f}"
            ),
            LogColor.BLUE,
        )

    def on_stop(self) -> None:
        self.unsubscribe_bars(self.config.bar_type)
        self.unsubscribe_quote_ticks(self.config.instrument_id)
        self.unsubscribe_trade_ticks(self.config.instrument_id)


def build_live_node(config: RithmicDataClientConfig, strategy: Strategy) -> TradingNode:
    node = TradingNode(
        config=TradingNodeConfig(
            trader_id=TraderId("TESTER-001"),
            logging=LoggingConfig(log_level="INFO", use_pyo3=True),
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            data_clients={RITHMIC: config},
            timeout_connection=10.0,
            timeout_disconnection=15.0,
            timeout_post_stop=2.0,
            timeout_shutdown=2.0,
        ),
    )
    node.trader.add_strategy(strategy)
    node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
    node.build()
    return node


# %% [markdown]
# ## Resolve The Current Contract

# %%
data_client_config = build_data_client_config(PROFILE, EXCHANGE)
front_month_contract, instrument = resolve_nautilus_instrument(
    data_client_config,
    PRODUCT,
    EXCHANGE,
)
live_bar_type = BarType.from_str(f"{instrument.id}-{LIVE_BAR_SPEC}")
live_data_client_config = build_data_client_config(
    PROFILE,
    EXCHANGE,
    load_ids=frozenset([instrument.id]),
)

instrument_summary = {
    "front_month_symbol": front_month_contract.symbol,
    "front_month_exchange": front_month_contract.exchange,
    "nautilus_instrument_id": str(instrument.id),
    "live_bar_type": str(live_bar_type),
    "price_increment": float(instrument.price_increment),
    "currency": instrument.quote_currency.code,
}
instrument_summary

if SCRIPT_MODE:
    print(instrument_summary)


# %% [markdown]
# ## Build The Live Probe Strategy

# %%
strategy = RithmicLiveProbe(
    config=LiveProbeConfig(
        instrument_id=instrument.id,
        bar_type=live_bar_type,
        fast_ema_period=FAST_EMA_PERIOD,
        slow_ema_period=SLOW_EMA_PERIOD,
        log_every_n_ticks=LOG_EVERY_N_TICKS,
        verbose_tick_logs=VERBOSE_TICK_LOGS,
    ),
)
node = build_live_node(live_data_client_config, strategy)

{
    "instrument_id": str(instrument.id),
    "bar_type": str(live_bar_type),
    "run_seconds": RUN_SECONDS,
    "quote_feed": True,
    "trade_feed": True,
    "internal_consolidator": True,
}


# %% [markdown]
# ## Run The Live Probe
#
# As a script, this auto-stops after `RUN_SECONDS`.
# In a notebook, set `RITHMIC_NOTEBOOK_RUN_SECONDS` to a positive value if you want auto-stop.

# %%
stop_timer = schedule_stop(node, RUN_SECONDS)

try:
    node.run()
except KeyboardInterrupt:
    node.stop()
finally:
    if stop_timer is not None:
        stop_timer.cancel()
    node.dispose()


# %% [markdown]
# ## Inspect Captured Live Data

# %%
live_summary = {
    "instrument_id": str(instrument.id),
    "bar_type": str(live_bar_type),
    "quotes_seen": strategy.quote_count,
    "trades_seen": strategy.trade_count,
    "bars_seen": strategy.bar_count,
    "last_quote": strategy.last_quote,
    "last_trade": strategy.last_trade,
}
live_summary

if SCRIPT_MODE:
    print(live_summary)


# %%
bars_df = pd.DataFrame(strategy.bar_snapshots)
bars_df


# %% [markdown]
# ## Next Steps
#
# - Keep `1-MINUTE-LAST-INTERNAL` when you want trade-driven internal bars.
# - Switch to `1-MINUTE-MID-INTERNAL` if you want quote-driven internal bars instead.
# - Increase `RITHMIC_NOTEBOOK_RUN_SECONDS` to watch several completed bars.
# - Replace `RithmicLiveProbe` with a real strategy once the feed and internal bar path look sane.

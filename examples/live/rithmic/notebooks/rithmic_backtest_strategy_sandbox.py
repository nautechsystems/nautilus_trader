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
# # Rithmic Backtest Strategy Sandbox
#
# Objective:
# - Resolve the current front month for a Rithmic product root.
# - Download historical 1-minute bars into a local Nautilus catalog through the Rithmic adapter.
# - Run a simple EMA-cross backtest from that catalog to validate the historical bar path.
#
# Note:
# - Rithmic historical quote and trade tick requests are not implemented yet in this adapter.
# - This sandbox therefore backtests from historical 1-minute bars requested through `request_bars`.
# - On basic Rithmic plans, historical API usage is typically capped at 20 GB per month.
# - Rithmic sends warning emails to the registered account email address as usage approaches the
#   limit or when their access rules are being breached. Ignoring those warnings can trigger
#   automatic temporary restrictions.

# %% [markdown]
# Note: Use the jupytext python package to open this file as a notebook in Jupyter.
# Also run `jupytext-config set-default-viewer` if you want `.py` notebook files to open as notebooks by default.

# %% [markdown]
# ## Imports

# %%
from __future__ import annotations

import asyncio
import os
import subprocess
import sys
import textwrap
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic.bindings import RithmicGateway
from nautilus_trader.adapters.rithmic.bindings import (
    RithmicInstrumentProvider as BindingInstrumentProvider,
)
from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import to_binding_environment
from nautilus_trader.adapters.rithmic.factories import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic.providers import (
    RithmicInstrumentProvider as PythonInstrumentProvider,
)
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.config import DataCatalogConfig

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
LOOKBACK_MINUTES = int(os.environ.get("RITHMIC_NOTEBOOK_LOOKBACK_MINUTES", "120"))
BAR_SPEC = "1-MINUTE-LAST"
CATALOG_PATH = Path(
    os.environ.get("RITHMIC_NOTEBOOK_CATALOG_PATH", "tmp/rithmic_notebook_catalog"),
).resolve()

{
    "profile": PROFILE,
    "product": PRODUCT,
    "exchange": EXCHANGE,
    "lookback_minutes": LOOKBACK_MINUTES,
    "bar_spec": BAR_SPEC,
    "catalog_path": str(CATALOG_PATH),
}


# %% [markdown]
# ## Helpers

# %%
def build_data_client_config(profile: str, exchange: str) -> RithmicDataClientConfig:
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
        enable_history=True,
        instrument_provider=InstrumentProviderConfig(
            load_all=False,
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


def download_bars_to_catalog(
    catalog_path: Path,
    instrument,
    exchange: str,
    lookback_minutes: int,
):
    end = pd.Timestamp.utcnow().floor("min")
    start = end - pd.Timedelta(minutes=lookback_minutes)
    bar_type = BarType.from_str(f"{instrument.id}-{BAR_SPEC}-EXTERNAL")
    script = textwrap.dedent(
        f"""
        from __future__ import annotations

        import asyncio
        from pathlib import Path

        import pandas as pd

        from nautilus_trader.adapters.rithmic import RITHMIC
        from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
        from nautilus_trader.adapters.rithmic.factories import RithmicLiveDataClientFactory
        from nautilus_trader.adapters.rithmic.providers import (
            RithmicInstrumentProvider as PythonInstrumentProvider,
        )
        from nautilus_trader.backtest.node import BacktestNode
        from nautilus_trader.config import InstrumentProviderConfig
        from nautilus_trader.model.data import BarType
        from nautilus_trader.model.identifiers import InstrumentId
        from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
        from nautilus_trader.persistence.config import DataCatalogConfig

        base = RithmicDataClientConfig.from_env({PROFILE!r})
        config = RithmicDataClientConfig(
            environment=base.environment,
            username=base.username,
            password=base.password,
            system_name=base.system_name,
            app_name=base.app_name,
            app_version=base.app_version,
            fcm_id=base.fcm_id,
            ib_id=base.ib_id,
            enable_history=True,
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                filters={{"exchange": {exchange!r}}},
            ),
        )
        provider = PythonInstrumentProvider(config)
        instrument_id = InstrumentId.from_str({str(instrument.id)!r})
        asyncio.run(provider.load_async(instrument_id, filters={{"exchange": {exchange!r}}}))
        instrument = provider.find(instrument_id)
        if instrument is None:
            raise RuntimeError(f"Failed to load {{instrument_id}}")
        gateway = getattr(provider, "_gateway", None)
        if gateway is not None and gateway.is_connected():
            async def disconnect_gateway():
                await gateway.disconnect()
            asyncio.run(disconnect_gateway())

        catalog_path = Path({str(catalog_path)!r}).resolve()
        catalog = ParquetDataCatalog(str(catalog_path))
        catalog.write_data([instrument])

        bar_type = BarType.from_str({str(bar_type)!r})
        start = pd.Timestamp({start.isoformat()!r})
        end = pd.Timestamp({end.isoformat()!r})

        node = BacktestNode([])
        node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
        node.setup_download_engine(
            catalog_config=DataCatalogConfig(path=str(catalog_path)),
            data_clients={{RITHMIC: config}},
        )
        node.download_data(
            "request_bars",
            bar_type=bar_type,
            start=start.to_pydatetime(),
            end=end.to_pydatetime(),
            params={{"exchange": {exchange!r}}},
        )
        node.dispose()
        """
    )
    subprocess.run(
        [sys.executable, "-c", script],
        check=True,
        cwd=Path.cwd(),
        env=os.environ.copy(),
    )

    return start, end, bar_type


def bars_to_frame(bars) -> pd.DataFrame:
    return pd.DataFrame(
        [
            {
                "open": float(bar.open),
                "high": float(bar.high),
                "low": float(bar.low),
                "close": float(bar.close),
                "volume": float(bar.volume),
            }
            for bar in bars
        ],
    )


def run_ema_backtest(instrument, bars):
    if not bars:
        raise RuntimeError("No bars found in catalog for backtest")

    engine = BacktestEngine(
        config=BacktestEngineConfig(
            trader_id=TraderId("BACKTESTER-001"),
            logging=LoggingConfig(log_level="ERROR", log_colors=False, use_pyo3=False),
            run_analysis=False,
        ),
    )
    venue = Venue(RITHMIC)
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=instrument.quote_currency,
        starting_balances=[Money(250_000.0, instrument.quote_currency)],
    )
    engine.add_instrument(instrument)
    engine.add_data(bars)
    engine.add_strategy(
        EMACross(
            config=EMACrossConfig(
                instrument_id=instrument.id,
                bar_type=bars[0].bar_type,
                trade_size=Decimal("1"),
                fast_ema_period=10,
                slow_ema_period=20,
                subscribe_trade_ticks=False,
                subscribe_quote_ticks=False,
                request_bars=False,
            ),
        ),
    )
    engine.run()

    results = {
        "fills": engine.trader.generate_order_fills_report(),
        "positions": engine.trader.generate_positions_report(),
        "account": engine.trader.generate_account_report(venue),
    }
    engine.dispose()
    return results


# %% [markdown]
# ## Resolve The Current Contract

# %%
data_client_config = build_data_client_config(PROFILE, EXCHANGE)
front_month_contract, instrument = resolve_nautilus_instrument(
    data_client_config,
    PRODUCT,
    EXCHANGE,
)

instrument_summary = {
    "front_month_symbol": front_month_contract.symbol,
    "front_month_exchange": front_month_contract.exchange,
    "nautilus_instrument_id": str(instrument.id),
    "price_increment": float(instrument.price_increment),
    "currency": instrument.quote_currency.code,
}
instrument_summary

if SCRIPT_MODE:
    print(instrument_summary)


# %% [markdown]
# ## Download Historical 1-Minute Bars Through The Adapter
#
# If Rithmic returns an empty response on a fresh session, rerun this cell once.
#
# Keep history requests measured. On basic Rithmic plans, historical API usage is typically capped
# at 20 GB per month, and Rithmic sends warning emails to the registered account email address when
# usage approaches the limit or their access rules are being breached. Ignoring those warnings can
# result in automatically triggered temporary restrictions.

# %%
window_start, window_end, bar_type = download_bars_to_catalog(
    CATALOG_PATH,
    instrument,
    EXCHANGE,
    LOOKBACK_MINUTES,
)

download_summary = {
    "catalog_path": str(CATALOG_PATH),
    "bar_type": str(bar_type),
    "window_start": window_start.isoformat(),
    "window_end": window_end.isoformat(),
}
download_summary

if SCRIPT_MODE:
    print(download_summary)


# %% [markdown]
# ## Inspect Catalog Contents

# %%
catalog = ParquetDataCatalog(str(CATALOG_PATH))
catalog_instrument = catalog.instruments(instrument_ids=[str(instrument.id)])[-1]
catalog_bars = catalog.bars(bar_types=[str(bar_type)])
bars_df = bars_to_frame(catalog_bars)
first_ts = catalog.query_first_timestamp(type(catalog_bars[0]), str(bar_type)) if catalog_bars else None
last_ts = catalog.query_last_timestamp(type(catalog_bars[0]), str(bar_type)) if catalog_bars else None

summary = {
    "instrument_id": str(catalog_instrument.id),
    "bar_type": str(bar_type),
    "bar_count": len(catalog_bars),
    "first_ts": first_ts,
    "last_ts": last_ts,
}
summary

if SCRIPT_MODE:
    print(summary)


# %%
bars_df.tail(10)


# %% [markdown]
# ## Run A Simple Strategy Backtest

# %%
backtest_results = run_ema_backtest(catalog_instrument, catalog_bars)

backtest_summary = {
    "fills": len(backtest_results["fills"]),
    "positions": len(backtest_results["positions"]),
    "ending_total": backtest_results["account"].iloc[-1]["total"],
}
backtest_summary

if SCRIPT_MODE:
    print(backtest_summary)


# %%
backtest_results["fills"]


# %%
backtest_results["positions"]


# %%
backtest_results["account"].tail(5)


# %% [markdown]
# ## Next Steps
#
# - Increase `LOOKBACK_MINUTES` when you want a longer offline backtest window.
# - Swap `PRODUCT` and `EXCHANGE` to validate another futures root.
# - Replace the `EMACross` strategy cell with your own strategy once the historical bar path looks sane.

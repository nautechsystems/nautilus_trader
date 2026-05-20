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
Deterministic Liquidation Engine Demo - NautilusTrader Issue #3788.

Demonstrates automatic margin liquidation using the Rust SimulatedExchange.

Run with:
    python examples/backtest/liquidation_demo.py
    python examples/backtest/liquidation_demo.py --json
"""

import json
import sys

from nautilus_trader.core.nautilus_pyo3.trading import ImportableStrategyConfig as _ISC
from nautilus_trader.core.nautilus_pyo3.backtest import BacktestEngine
from nautilus_trader.core.nautilus_pyo3.backtest import BacktestEngineConfig
from nautilus_trader.core.nautilus_pyo3.model import AccountType
from nautilus_trader.core.nautilus_pyo3.model import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3.model import Currency
from nautilus_trader.core.nautilus_pyo3.model import Money
from nautilus_trader.core.nautilus_pyo3.model import OmsType
from nautilus_trader.core.nautilus_pyo3.model import Price
from nautilus_trader.core.nautilus_pyo3.model import Quantity
from nautilus_trader.core.nautilus_pyo3.model import QuoteTick
from nautilus_trader.core.nautilus_pyo3.model import Venue
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTC = Currency.from_str("BTC")
BITMEX = Venue("BITMEX")
_xbtusd_cython = TestInstrumentProvider.xbtusd_bitmex()
XBTUSD = CryptoPerpetual.from_dict(_xbtusd_cython.to_dict(_xbtusd_cython))

_STEPS: list[str] = []


def _log(msg: str) -> None:
    _STEPS.append(msg)
    print(msg, flush=True)


def _make_quote(price: float, ts: int = 0) -> QuoteTick:
    p = Price.from_str(f"{price:.1f}")
    return QuoteTick(
        instrument_id=XBTUSD.id,
        bid_price=p,
        ask_price=p,
        bid_size=Quantity.from_int(10_000_000),
        ask_size=Quantity.from_int(10_000_000),
        ts_event=ts,
        ts_init=ts,
    )


def run_demo() -> dict:
    _STEPS.clear()

    _log("-" * 60)
    _log("  NautilusTrader - Deterministic Liquidation Engine Demo")
    _log("  GitHub Issue #3788")
    _log("-" * 60)

    ENTRY_PRICE = 40_000.0
    CRASH_PRICE = 20_000.0
    STARTING_BTC = 1.0
    QUANTITY = 10_000_000

    _log("\n[CONFIG]")
    _log("  Exchange      : BITMEX  (XBTUSD inverse perpetual)")
    _log("  Leverage      : 100x (default)")
    _log(f"  Starting BTC  : {STARTING_BTC} BTC")
    _log("  Liquidation   : ENABLED  (trigger_ratio=1.0)")

    engine = BacktestEngine(config=BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=BITMEX,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=BTC,
        starting_balances=[Money(STARTING_BTC, BTC)],
        liquidation_enabled=True,
        liquidation_trigger_ratio=1.0,
        liquidation_cancel_open_orders=True,
    )
    engine.add_instrument(XBTUSD)

    engine.add_strategy_from_config(
        _ISC(
            strategy_path=(
                "nautilus_trader.examples.strategies.market_buy_on_start:MarketBuyOnStart"
            ),
            config_path=(
                "nautilus_trader.examples.strategies.market_buy_on_start:MarketBuyOnStartConfig"
            ),
            config={
                "instrument_id": str(XBTUSD.id),
                "trade_size": QUANTITY,
            },
        )
    )

    ticks = [
        _make_quote(ENTRY_PRICE, ts=0),
        _make_quote(CRASH_PRICE, ts=1),
    ]
    engine.add_data(ticks)

    _log(f"\n[STEP 1] Market opens @ ${ENTRY_PRICE:,.0f}")
    _log(f"  Strategy will submit BUY {QUANTITY:,} XBTUSD contracts on first tick")
    _log(f"\n[STEP 2] Price crashes from ${ENTRY_PRICE:,.0f} to ${CRASH_PRICE:,.0f} (-50%)")

    engine.run()

    cache = engine.cache
    open_positions = cache.positions_open_count()
    closed_positions = cache.positions_closed_count()

    _log("\n[RESULT]")
    if open_positions == 0 and closed_positions >= 1:
        _log("  LIQUIDATION TRIGGERED")
        _log("  All positions closed by engine")
    else:
        _log("  Liquidation did NOT fire (unexpected)")

    _log(f"  Open positions  : {open_positions}")
    _log(f"  Closed positions: {closed_positions}")
    _log("\n" + "-" * 60)

    result = {
        "config": {
            "entry_price": ENTRY_PRICE,
            "crash_price": CRASH_PRICE,
            "starting_btc": STARTING_BTC,
            "quantity_contracts": QUANTITY,
            "liquidation_enabled": True,
            "liquidation_trigger_ratio": 1.0,
            "liquidation_cancel_open_orders": True,
        },
        "result": {
            "liquidation_triggered": open_positions == 0 and closed_positions >= 1,
            "open_positions_after": open_positions,
            "closed_positions_after": closed_positions,
        },
        "log": _STEPS[:],
    }

    engine.dispose()
    return result


if __name__ == "__main__":
    result = run_demo()
    if "--json" in sys.argv:
        print(json.dumps(result, indent=2))

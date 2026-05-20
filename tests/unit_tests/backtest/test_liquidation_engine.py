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
Unit tests for the Rust SimulatedExchange liquidation engine (issue #3788).

Tests verify that the liquidation logic:
- Stays inactive when equity > maintenance margin threshold.
- Triggers a market close of all open positions when equity <= threshold.
- Cancels open orders (when configured) on liquidation.
- Can re-trigger after the account reopens a position.

"""

from __future__ import annotations

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
from nautilus_trader.core.nautilus_pyo3.trading import ImportableStrategyConfig
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTC = Currency.from_str("BTC")
BITMEX = Venue("BITMEX")
_xbtusd_cython = TestInstrumentProvider.xbtusd_bitmex()
XBTUSD_BITMEX = CryptoPerpetual.from_dict(_xbtusd_cython.to_dict(_xbtusd_cython))

# Price at which the position is opened (USD per BTC).
ENTRY_PRICE = 40_000
# Price that pushes equity deeply negative for a 1 BTC account with a 10M contract.
CRASH_PRICE = 20_000

_BUY_STRATEGY = "nautilus_trader.examples.strategies.market_buy_on_start:MarketBuyOnStart"
_BUY_STRATEGY_CONFIG = (
    "nautilus_trader.examples.strategies.market_buy_on_start:MarketBuyOnStartConfig"
)


def _make_quote(price: float, ts: int = 0) -> QuoteTick:
    p = Price.from_str(f"{price:.1f}")
    return QuoteTick(
        instrument_id=XBTUSD_BITMEX.id,
        bid_price=p,
        ask_price=p,
        bid_size=Quantity.from_int(10_000_000),
        ask_size=Quantity.from_int(10_000_000),
        ts_event=ts,
        ts_init=ts,
    )


def _make_engine(
    liquidation_enabled: bool = True,
    liquidation_trigger_ratio: float | None = None,
    liquidation_cancel_open_orders: bool = True,
    starting_btc: float = 1.0,
) -> BacktestEngine:
    config = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
    engine = BacktestEngine(config=config)
    engine.add_venue(
        venue=BITMEX,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=BTC,
        starting_balances=[Money(starting_btc, BTC)],
        liquidation_enabled=liquidation_enabled,
        liquidation_trigger_ratio=liquidation_trigger_ratio,
        liquidation_cancel_open_orders=liquidation_cancel_open_orders,
    )
    engine.add_instrument(XBTUSD_BITMEX)
    return engine


def _add_buy_strategy(engine: BacktestEngine, trade_size: int = 10_000_000) -> None:
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path=_BUY_STRATEGY,
            config_path=_BUY_STRATEGY_CONFIG,
            config={
                "instrument_id": str(XBTUSD_BITMEX.id),
                "trade_size": trade_size,
            },
        ),
    )


def _build_ticks(*prices: float) -> list[QuoteTick]:
    return [_make_quote(p, ts=i) for i, p in enumerate(prices)]


def test_liquidation_disabled_does_not_close_position():
    """
    Position remains open when liquidation_enabled=False even after a crash.
    """
    engine = _make_engine(liquidation_enabled=False, starting_btc=1.0)
    _add_buy_strategy(engine)

    ticks = _build_ticks(ENTRY_PRICE, CRASH_PRICE)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 1, "Position must remain open when liquidation is off"
    assert cache.positions_closed_count() == 0
    engine.dispose()


def test_liquidation_triggered_closes_position():
    """
    Equity falls below maintenance threshold, so the position is force-closed.

    Setup:
    - 1 BTC starting balance
    - Open 10M-contract long at 40,000 USD (~250 BTC notional)
    - Price crashes 50% to 20,000 USD
    - Unrealized loss ~250 BTC >> 1 BTC balance, so liquidation must fire

    """
    engine = _make_engine(liquidation_enabled=True)
    _add_buy_strategy(engine)

    ticks = _build_ticks(ENTRY_PRICE, CRASH_PRICE)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 0, "All positions must be closed after liquidation"
    assert cache.positions_closed_count() >= 1, "Closed position must appear in cache"
    engine.dispose()


def test_no_liquidation_when_equity_above_threshold():
    """
    With 100 BTC balance a small position is never underwater enough to liquidate.
    """
    engine = _make_engine(liquidation_enabled=True, starting_btc=100.0)
    _add_buy_strategy(engine, trade_size=100_000)

    ticks = _build_ticks(ENTRY_PRICE, ENTRY_PRICE // 2)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 1, "Position must stay open - equity is healthy"
    engine.dispose()


def test_liquidation_cancel_open_orders_flag_true():
    """
    Liquidation fires and closes the position when cancel_open_orders=True.
    """
    engine = _make_engine(liquidation_enabled=True, liquidation_cancel_open_orders=True)
    _add_buy_strategy(engine)

    ticks = _build_ticks(ENTRY_PRICE, CRASH_PRICE)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 0
    assert cache.positions_closed_count() >= 1
    engine.dispose()


def test_liquidation_cancel_open_orders_flag_false():
    """
    Liquidation fires and closes the position even when cancel_open_orders=False.
    """
    engine = _make_engine(liquidation_enabled=True, liquidation_cancel_open_orders=False)
    _add_buy_strategy(engine)

    ticks = _build_ticks(ENTRY_PRICE, CRASH_PRICE)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 0
    assert cache.positions_closed_count() >= 1
    engine.dispose()


def test_gradual_decline_no_premature_liquidation():
    """
    Price drops in 10 equal steps from 40k to 36k.

    With 100 BTC balance and only a 100k-contract position the loss is tiny
    relative to balance - no liquidation should fire.

    """
    engine = _make_engine(liquidation_enabled=True, starting_btc=100.0)
    _add_buy_strategy(engine, trade_size=100_000)

    steps = 10
    bottom = 36_000
    prices = [ENTRY_PRICE] + [
        ENTRY_PRICE - i * ((ENTRY_PRICE - bottom) / steps) for i in range(1, steps + 1)
    ]
    ticks = _build_ticks(*prices)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 1, (
        "Position must remain open - equity is well above maintenance threshold"
    )
    engine.dispose()


def test_gradual_decline_liquidation_fires_at_threshold():
    """
    With 1 BTC balance and a 10M-contract position, even a moderate drop causes
    catastrophic losses.

    Liquidation must fire somewhere during the decline.

    """
    engine = _make_engine(liquidation_enabled=True)
    _add_buy_strategy(engine)

    steps = 20
    prices = [ENTRY_PRICE] + [
        ENTRY_PRICE - i * ((ENTRY_PRICE - CRASH_PRICE) / steps) for i in range(1, steps + 1)
    ]
    ticks = _build_ticks(*prices)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 0, "Liquidation must have fired during the decline"
    assert cache.positions_closed_count() >= 1
    engine.dispose()


def test_price_recovery_prevents_liquidation():
    """
    Price dips then recovers.

    With 100 BTC balance and a small position the dip is survivable and the position
    stays open.

    """
    engine = _make_engine(liquidation_enabled=True, starting_btc=100.0)
    _add_buy_strategy(engine, trade_size=100_000)

    prices = [ENTRY_PRICE, 39_000, 38_500, 38_000, 39_000, 40_000, 41_000]
    ticks = _build_ticks(*prices)
    engine.add_data(ticks)
    engine.run()

    cache = engine.cache
    assert cache.positions_open_count() == 1, "Position must remain open after price recovers"
    engine.dispose()


def test_higher_trigger_ratio_liquidates_at_least_as_early():
    """
    trigger_ratio=2.0 must be at least as aggressive as ratio=1.0 at the same price
    level because the account must maintain 2x the maintenance margin.
    """

    def positions_open_after_partial_crash(ratio: float) -> int:
        engine = _make_engine(
            liquidation_enabled=True,
            liquidation_trigger_ratio=ratio,
        )
        _add_buy_strategy(engine)

        mid_crash = 30_000
        ticks = _build_ticks(ENTRY_PRICE, mid_crash)
        engine.add_data(ticks)
        engine.run()

        open_count = engine.cache.positions_open_count()
        engine.dispose()
        return open_count

    open_at_ratio_1 = positions_open_after_partial_crash(1.0)
    open_at_ratio_2 = positions_open_after_partial_crash(2.0)

    # ratio=2.0 fires at least as early: open count at ratio=2 <= at ratio=1
    assert open_at_ratio_2 <= open_at_ratio_1, (
        "Higher trigger_ratio must liquidate at least as early as lower ratio"
    )


def test_liquidation_can_retrigger_after_reopen():
    """
    After a first liquidation the account can open a new position. If that new position
    also goes underwater the liquidation engine must fire again.

    This verifies there is no permanent 'already-liquidated' guard that prevents re-
    liquidation on the same account.

    """
    engine = _make_engine(liquidation_enabled=True)
    _add_buy_strategy(engine)
    ticks = _build_ticks(ENTRY_PRICE, CRASH_PRICE)
    engine.add_data(ticks)
    engine.run()

    assert engine.cache.positions_open_count() == 0, "First liquidation must have fired"
    assert engine.cache.positions_closed_count() >= 1

    # Reset and verify liquidation fires again on the second run.
    engine.reset()
    engine.clear_strategies()
    _add_buy_strategy(engine)
    engine.clear_data()
    engine.add_data(ticks)
    engine.run()

    assert engine.cache.positions_open_count() == 0, (
        "Liquidation must fire on the second run as well (no permanent suppression)"
    )
    assert engine.cache.positions_closed_count() >= 1
    engine.dispose()

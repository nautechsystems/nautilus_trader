#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Deterministic Liquidation Engine Demo — NautilusTrader Issue #3788
#
#  Runs a tick-by-tick market simulation that demonstrates automatic margin
#  liquidation.  Designed to be called by the web dashboard and to produce
#  human-readable output on stdout.
# -------------------------------------------------------------------------------------------------

import json
import sys
from decimal import Decimal

from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


XBTUSD = TestInstrumentProvider.xbtusd_bitmex()

STEPS = []


def log(msg: str) -> None:
    STEPS.append(msg)
    print(msg, flush=True)


def make_quote(price: float, ts: int = 0) -> QuoteTick:
    p = f"{price:.1f}"
    return QuoteTick(
        instrument_id=XBTUSD.id,
        bid_price=Price.from_str(p),
        ask_price=Price.from_str(p),
        bid_size=Quantity.from_int(10_000_000),
        ask_size=Quantity.from_int(10_000_000),
        ts_event=ts,
        ts_init=ts,
    )


def build_exchange(
    liquidation_enabled: bool,
    liquidation_trigger_ratio: float,
    starting_btc: float,
) -> tuple[SimulatedExchange, DataEngine, ExecutionEngine, MockStrategy, Cache]:
    clock = TestClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    cache = TestComponentStubs.cache()
    portfolio = Portfolio(msgbus=msgbus, cache=cache, clock=clock)
    data_engine = DataEngine(msgbus=msgbus, cache=cache, clock=clock)
    exec_engine = ExecutionEngine(msgbus=msgbus, cache=cache, clock=clock)
    RiskEngine(portfolio=portfolio, msgbus=msgbus, cache=cache, clock=clock)

    exchange = SimulatedExchange(
        venue=Venue("BITMEX"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=BTC,
        starting_balances=[Money(starting_btc, BTC)],
        default_leverage=Decimal(100),
        leverages={},
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        modules=[],
        fill_model=FillModel(),
        fee_model=MakerTakerFeeModel(),
        clock=clock,
        latency_model=LatencyModel(0),
        liquidation_enabled=liquidation_enabled,
        liquidation_trigger_ratio=liquidation_trigger_ratio,
        liquidation_cancel_open_orders=True,
    )
    exchange.add_instrument(XBTUSD)

    exec_client = BacktestExecClient(exchange=exchange, msgbus=msgbus, cache=cache, clock=clock)
    exec_engine.register_client(exec_client)
    exchange.register_client(exec_client)
    cache.add_instrument(XBTUSD)

    strategy = MockStrategy(bar_type=TestDataStubs.bartype_btcusdt_binance_100tick_last())
    strategy.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exchange.reset()
    data_engine.start()
    exec_engine.start()
    strategy.start()

    return exchange, data_engine, exec_engine, strategy, cache


def run_demo() -> dict:
    STEPS.clear()

    log("=" * 60)
    log("  NautilusTrader — Deterministic Liquidation Engine Demo")
    log("  GitHub Issue #3788")
    log("=" * 60)

    # ── Configuration ────────────────────────────────────────────
    ENTRY_PRICE = 40_000.0
    CRASH_PRICE = 20_000.0
    STARTING_BTC = 1.0
    QUANTITY = 10_000_000  # contracts (10M USD notional @ $40k = ~250 BTC notional)

    log("\n[CONFIG]")
    log("  Exchange      : BITMEX  (XBTUSD inverse perpetual)")
    log("  Leverage      : 100x")
    log(f"  Starting BTC  : {STARTING_BTC} BTC")
    log("  Liquidation   : ENABLED  (trigger_ratio=1.0)")

    # ── Build exchange ───────────────────────────────────────────
    exchange, data_engine, exec_engine, strategy, cache = build_exchange(
        liquidation_enabled=True,
        liquidation_trigger_ratio=1.0,
        starting_btc=STARTING_BTC,
    )

    # ── Step 1: Market opens at $40,000 ─────────────────────────
    log("\n[STEP 1] Market opens @ $40,000")
    q1 = make_quote(ENTRY_PRICE, ts=1)
    data_engine.process(q1)
    exchange.process_quote_tick(q1)

    order = strategy.order_factory.market(
        XBTUSD.id,
        OrderSide.BUY,
        Quantity.from_int(QUANTITY),
    )
    strategy.submit_order(order)
    exchange.process(1)

    orders_after_open = exchange.get_open_orders()
    positions_after_open = cache.positions_open()

    log(f"  → Submitted BUY {QUANTITY:,} XBTUSD contracts")
    log(f"  → Open orders   : {len(orders_after_open)}")
    log(f"  → Open positions: {len(positions_after_open)}")

    # ── Step 2: Add a pending limit sell (to verify cancel-on-liquidate) ──
    log("\n[STEP 2] Place a limit SELL order (will be cancelled on liquidation)")
    limit_order = strategy.order_factory.limit(
        XBTUSD.id,
        OrderSide.SELL,
        Quantity.from_int(100_000),
        Price.from_str("45000.0"),
    )
    strategy.submit_order(limit_order)
    exchange.process(2)
    open_before = exchange.get_open_orders()
    log(f"  → Open orders before crash: {len(open_before)}")

    # ── Step 3: Price crashes to $20,000 ────────────────────────
    log(f"\n[STEP 3] Price crashes from ${ENTRY_PRICE:,.0f} → ${CRASH_PRICE:,.0f}  (-50%)")
    q2 = make_quote(CRASH_PRICE, ts=3)
    data_engine.process(q2)
    exchange.process_quote_tick(q2)
    exchange.process(3)

    orders_after = exchange.get_open_orders()
    positions_after = cache.positions_open()

    log(f"  → Open orders after crash  : {len(orders_after)}  (was {len(open_before)})")
    log(f"  → Open positions after crash: {len(positions_after)}")

    # ── Result ───────────────────────────────────────────────────
    liquidation_fired = len(positions_after) == 0 and len(orders_after) == 0

    log("\n[RESULT]")
    if liquidation_fired:
        log("  ✓ LIQUIDATION TRIGGERED")
        log("  ✓ All positions closed by engine")
        log("  ✓ All pending orders cancelled")
    else:
        log("  ✗ Liquidation did NOT fire (unexpected)")

    log("\n" + "=" * 60)

    return {
        "config": {
            "entry_price": ENTRY_PRICE,
            "crash_price": CRASH_PRICE,
            "starting_btc": STARTING_BTC,
            "leverage": 100,
            "quantity_contracts": QUANTITY,
            "liquidation_enabled": True,
            "liquidation_trigger_ratio": 1.0,
        },
        "result": {
            "liquidation_triggered": liquidation_fired,
            "open_positions_after": len(positions_after),
            "open_orders_after": len(orders_after),
        },
        "log": STEPS[:],
    }


if __name__ == "__main__":
    result = run_demo()
    if "--json" in sys.argv:
        print(json.dumps(result, indent=2))

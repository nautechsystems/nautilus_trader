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

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMaker
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMakerConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
_AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")

# AUD/USD: price_precision=5, size_precision=0, min_quantity=1000
_TRADE_SIZE = Quantity.from_int(1000)
# Large enough to place a full 3-level replacement grid while 3 pending-cancel orders
# still count against worst-case exposure (_compute_exposure includes inflight buys/sells)
_MAX_POSITION = Quantity.from_int(7000)


@pytest.fixture
def env():
    """
    Full backtest environment wired for GridMarketMaker tests.
    """
    clock = TestClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    cache = TestComponentStubs.cache()
    portfolio = Portfolio(msgbus=msgbus, cache=cache, clock=clock)
    data_engine = DataEngine(msgbus=msgbus, cache=cache, clock=clock)
    exec_engine = ExecutionEngine(msgbus=msgbus, cache=cache, clock=clock)
    RiskEngine(portfolio=portfolio, msgbus=msgbus, cache=cache, clock=clock)

    instrument = _AUDUSD
    exchange = SimulatedExchange(
        venue=SIM,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        default_leverage=Decimal(50),
        leverages={},
        modules=[],
        fill_model=FillModel(),
        fee_model=MakerTakerFeeModel(),
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        latency_model=LatencyModel(0),
    )
    exchange.add_instrument(instrument)
    cache.add_instrument(instrument)

    data_client = BacktestMarketDataClient(
        client_id=ClientId("SIM"),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    exec_client = BacktestExecClient(
        exchange=exchange,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    exchange.register_client(exec_client)
    data_engine.register_client(data_client)
    exec_engine.register_client(exec_client)
    exchange.reset()
    data_engine.start()
    exec_engine.start()

    return SimpleNamespace(
        clock=clock,
        trader_id=trader_id,
        msgbus=msgbus,
        cache=cache,
        portfolio=portfolio,
        data_engine=data_engine,
        exec_engine=exec_engine,
        exchange=exchange,
        instrument=instrument,
    )


def _make_strategy(env, **overrides) -> GridMarketMaker:
    defaults = {
        "instrument_id": env.instrument.id,
        "max_position": _MAX_POSITION,
        "trade_size": _TRADE_SIZE,
        "num_levels": 3,
        "grid_step_bps": 10,
        "skew_factor": 0.0,
        "requote_threshold_bps": 5,
    }
    defaults.update(overrides)
    config = GridMarketMakerConfig(**defaults)
    strategy = GridMarketMaker(config=config)
    strategy.register(
        trader_id=env.trader_id,
        portfolio=env.portfolio,
        msgbus=env.msgbus,
        cache=env.cache,
        clock=env.clock,
    )
    return strategy


def _process_quote(env, bid_price: float, ask_price: float) -> None:
    ts = env.clock.timestamp_ns()
    tick = TestDataStubs.quote_tick(
        instrument=env.instrument,
        bid_price=bid_price,
        ask_price=ask_price,
        ts_event=ts,
        ts_init=ts,
    )
    env.data_engine.process(tick)
    env.exchange.process_quote_tick(tick)
    env.exchange.process(ts)


def test_init_defaults(env):
    # Act
    strategy = _make_strategy(env)

    # Assert
    assert strategy.config.num_levels == 3
    assert strategy.config.grid_step_bps == 10
    assert strategy.config.skew_factor == 0.0
    assert strategy.config.requote_threshold_bps == 5
    assert strategy.config.expire_time_secs is None
    assert strategy.config.on_cancel_resubmit is False


def test_start_loads_instrument(env):
    # Arrange
    strategy = _make_strategy(env)

    # Act
    strategy.start()

    # Assert
    assert strategy._instrument is not None
    assert strategy._instrument.id == env.instrument.id
    assert strategy._price_precision == env.instrument.price_precision


def test_start_resolves_trade_size_from_min_quantity(env):
    # Arrange — min_quantity for AUD/USD.SIM is 1000
    strategy = _make_strategy(env, trade_size=None)

    # Act
    strategy.start()

    # Assert
    assert strategy._trade_size == env.instrument.min_quantity


def test_first_quote_places_full_grid(env):
    # Arrange
    strategy = _make_strategy(env, num_levels=3)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    assert len(orders) == 6


def test_first_quote_places_equal_buys_and_sells(env):
    # Arrange
    strategy = _make_strategy(env, num_levels=3)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    buys = [o for o in orders if o.side == OrderSide.BUY]
    sells = [o for o in orders if o.side == OrderSide.SELL]
    assert len(buys) == 3
    assert len(sells) == 3


def test_buy_prices_below_mid(env):
    # Arrange
    strategy = _make_strategy(env, num_levels=3)
    strategy.start()
    mid = Price(0.65000, env.instrument.price_precision)

    # Act — mid = (0.64990 + 0.65010) / 2 = 0.65000
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    buy_prices = [o.price for o in orders if o.side == OrderSide.BUY]
    assert all(p < mid for p in buy_prices)


def test_sell_prices_above_mid(env):
    # Arrange
    strategy = _make_strategy(env, num_levels=3)
    strategy.start()
    mid = Price(0.65000, env.instrument.price_precision)

    # Act — mid = 0.65000
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    sell_prices = [o.price for o in orders if o.side == OrderSide.SELL]
    assert all(p > mid for p in sell_prices)


def test_grid_uses_geometric_spacing(env):
    # Arrange — grid_step_bps=10: level N price = mid * (1 ± 0.001)^N
    strategy = _make_strategy(env, num_levels=2, grid_step_bps=10)
    strategy.start()
    mid = 0.65000
    pct = 0.001
    precision = env.instrument.price_precision

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    buy_prices = sorted([float(o.price) for o in orders if o.side == OrderSide.BUY], reverse=True)
    sell_prices = sorted([float(o.price) for o in orders if o.side == OrderSide.SELL])
    expected_buys = [round(mid * (1 - pct) ** n, precision) for n in (1, 2)]
    expected_sells = [round(mid * (1 + pct) ** n, precision) for n in (1, 2)]
    assert buy_prices == pytest.approx(expected_buys, abs=10**-precision)
    assert sell_prices == pytest.approx(expected_sells, abs=10**-precision)


def test_orders_are_post_only_gtc_by_default(env):
    # Arrange
    strategy = _make_strategy(env)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    assert all(o.is_post_only for o in orders)
    assert all(o.time_in_force == TimeInForce.GTC for o in orders)


def test_orders_are_gtd_when_expire_time_set(env):
    # Arrange
    strategy = _make_strategy(env, expire_time_secs=60)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    assert all(o.time_in_force == TimeInForce.GTD for o in orders)
    assert all(o.expire_time is not None for o in orders)


def test_anchor_set_after_first_quote(env):
    # Arrange
    strategy = _make_strategy(env)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    assert strategy._last_quoted_mid == Price(0.65000, env.instrument.price_precision)


def test_requote_suppressed_below_threshold(env):
    # Arrange — requote_threshold_bps=5, first quote sets the anchor at mid=0.65000
    strategy = _make_strategy(env, requote_threshold_bps=5)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    first_orders = {o.client_order_id for o in env.cache.orders_open()}

    # Act — mid moves to 0.65019, ~2.9 bps < 5 bps threshold
    _process_quote(env, bid_price=0.65014, ask_price=0.65024)

    # Assert
    second_orders = {o.client_order_id for o in env.cache.orders_open()}
    assert first_orders == second_orders


def test_requote_triggered_above_threshold(env):
    # Arrange — first quote sets the anchor at mid=0.65000
    strategy = _make_strategy(env, requote_threshold_bps=5, num_levels=3)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    first_orders = {o.client_order_id for o in env.cache.orders_open()}

    # Act — mid moves to 0.65039, ~6.0 bps > 5 bps threshold
    _process_quote(env, bid_price=0.65034, ask_price=0.65044)

    # Assert
    second_orders = {o.client_order_id for o in env.cache.orders_open()}
    assert first_orders.isdisjoint(second_orders)
    assert len(second_orders) == 6


def test_old_orders_cancelled_on_requote(env):
    # Arrange
    strategy = _make_strategy(env, num_levels=3)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Act — trigger a requote
    _process_quote(env, bid_price=0.65034, ask_price=0.65044)

    # Assert — 6 old (cancelled) + 6 new (open)
    all_orders = env.cache.orders(instrument_id=env.instrument.id)
    open_orders = env.cache.orders_open(instrument_id=env.instrument.id)
    assert len(all_orders) == 12
    assert len(open_orders) == 6


def test_requote_bypasses_threshold_when_no_orders_resting(env):
    # Arrange — place orders then cancel them all externally; with on_cancel_resubmit=False
    # the anchor is NOT reset, so the threshold would normally suppress the next quote
    strategy = _make_strategy(env, requote_threshold_bps=5, on_cancel_resubmit=False)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    strategy.cancel_all_orders(env.instrument.id)
    env.exchange.process(env.clock.timestamp_ns())

    # Act — mid moves by ~0.3 bps (well below 5 bps threshold) with no resting orders
    _process_quote(env, bid_price=0.64992, ask_price=0.65012)

    # Assert — grid re-placed despite being within threshold
    assert len(env.cache.orders_open(instrument_id=env.instrument.id)) > 0


def test_max_position_limits_buy_count(env):
    # Arrange — max_position = 1 trade_size, so only 1 buy level fits
    strategy = _make_strategy(env, max_position=_TRADE_SIZE, num_levels=3)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    buys = [o for o in orders if o.side == OrderSide.BUY]
    assert len(buys) == 1


def test_max_position_limits_sell_count(env):
    # Arrange
    strategy = _make_strategy(env, max_position=_TRADE_SIZE, num_levels=3)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert
    orders = env.cache.orders_open(instrument_id=env.instrument.id)
    sells = [o for o in orders if o.side == OrderSide.SELL]
    assert len(sells) == 1


def test_anchor_not_advanced_when_no_orders_placed(env):
    # Arrange — max_position < trade_size so no orders can be placed
    strategy = _make_strategy(env, max_position=Quantity.from_int(500), num_levels=3)
    strategy.start()

    # Act
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Assert — anchor must not advance so the next tick can retry
    assert strategy._last_quoted_mid is None
    assert len(env.cache.orders_open()) == 0


def test_skew_lowers_grid_on_long_position(env):
    # Arrange
    strategy = _make_strategy(env, skew_factor=0.0001, num_levels=1)
    strategy.start()
    mid = Price(0.65000, env.instrument.price_precision)
    baseline = strategy._grid_orders(
        mid,
        net_position=0.0,
        worst_long=Decimal(0),
        worst_short=Decimal(0),
    )

    # Act
    skewed = strategy._grid_orders(
        mid,
        net_position=1000.0,
        worst_long=Decimal(0),
        worst_short=Decimal(0),
    )

    # Assert — positive net position shifts all prices down to discourage buying
    baseline_buy = next(p for side, p in baseline if side == OrderSide.BUY)
    baseline_sell = next(p for side, p in baseline if side == OrderSide.SELL)
    skewed_buy = next(p for side, p in skewed if side == OrderSide.BUY)
    skewed_sell = next(p for side, p in skewed if side == OrderSide.SELL)
    assert skewed_buy < baseline_buy
    assert skewed_sell < baseline_sell


def test_skew_raises_grid_on_short_position(env):
    # Arrange
    strategy = _make_strategy(env, skew_factor=0.0001, num_levels=1)
    strategy.start()
    mid = Price(0.65000, env.instrument.price_precision)
    baseline = strategy._grid_orders(
        mid,
        net_position=0.0,
        worst_long=Decimal(0),
        worst_short=Decimal(0),
    )

    # Act
    skewed = strategy._grid_orders(
        mid,
        net_position=-1000.0,
        worst_long=Decimal(0),
        worst_short=Decimal(0),
    )

    # Assert — negative net position shifts all prices up to discourage selling
    baseline_buy = next(p for side, p in baseline if side == OrderSide.BUY)
    skewed_buy = next(p for side, p in skewed if side == OrderSide.BUY)
    assert skewed_buy > baseline_buy


def test_external_cancel_resets_anchor_when_configured(env):
    # Arrange
    strategy = _make_strategy(env, on_cancel_resubmit=True)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    external_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]

    # Act — simulate an external cancel (order not in pending_self_cancels)
    event = TestEventStubs.order_canceled(external_order)
    strategy.on_order_canceled(event)

    # Assert
    assert strategy._last_quoted_mid is None


def test_self_cancel_does_not_reset_anchor(env):
    # Arrange
    strategy = _make_strategy(env, on_cancel_resubmit=True, num_levels=3)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Act — requote adds old orders to pending_self_cancels before cancelling them
    _process_quote(env, bid_price=0.65034, ask_price=0.65044)

    # Assert — self-cancels consumed from the set, anchor remains at new mid
    assert strategy._last_quoted_mid is not None
    assert strategy._pending_self_cancels == set()


def test_partial_fill_then_self_cancel_does_not_reset_anchor(env):
    # Arrange — mark an open order as a pending self-cancel (as the requote cycle would)
    strategy = _make_strategy(env, on_cancel_resubmit=True)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    target_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]
    strategy._pending_self_cancels.add(target_order.client_order_id)
    anchor_before = strategy._last_quoted_mid

    # Act — partial fill followed by a cancel of the remaining quantity
    partial_fill = TestEventStubs.order_filled(
        target_order,
        instrument=env.instrument,
        last_qty=Quantity.from_int(500),  # half of trade_size
    )
    strategy.on_order_filled(partial_fill)  # must not remove ID from pending_self_cancels
    cancel_event = TestEventStubs.order_canceled(target_order)
    strategy.on_order_canceled(cancel_event)

    # Assert — cancel recognised as self-cancel, anchor unchanged
    assert strategy._last_quoted_mid == anchor_before


def test_gtd_expiry_resets_anchor(env):
    # Arrange
    strategy = _make_strategy(env, expire_time_secs=60)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    gtd_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]

    # Act
    strategy.on_order_expired(TestEventStubs.order_expired(gtd_order))

    # Assert — grid is gone after expiry; anchor must reset to allow re-quoting
    assert strategy._last_quoted_mid is None


def test_gtd_expiry_clears_pending_self_cancel_id(env):
    # Arrange
    strategy = _make_strategy(env, expire_time_secs=60, on_cancel_resubmit=True)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    gtd_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]
    strategy._pending_self_cancels.add(gtd_order.client_order_id)

    # Act
    strategy.on_order_expired(TestEventStubs.order_expired(gtd_order))

    # Assert
    assert gtd_order.client_order_id not in strategy._pending_self_cancels


def test_rejection_resets_anchor(env):
    # Arrange
    strategy = _make_strategy(env)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    rejected_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]

    # Act — simulate venue rejecting an order (e.g. post-only collision)
    strategy.on_order_rejected(TestEventStubs.order_rejected(rejected_order))

    # Assert — anchor reset so next tick can retry placing the full grid
    assert strategy._last_quoted_mid is None


def test_rejection_clears_pending_self_cancel_id(env):
    # Arrange
    strategy = _make_strategy(env, on_cancel_resubmit=True)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    rejected_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]
    strategy._pending_self_cancels.add(rejected_order.client_order_id)

    # Act
    strategy.on_order_rejected(TestEventStubs.order_rejected(rejected_order))

    # Assert
    assert rejected_order.client_order_id not in strategy._pending_self_cancels


def test_full_fill_clears_pending_self_cancel_id(env):
    # Arrange — manually apply a full fill to an order so order.is_closed is True
    strategy = _make_strategy(env, on_cancel_resubmit=True, num_levels=1)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    target_order = env.cache.orders_open(instrument_id=env.instrument.id)[0]
    strategy._pending_self_cancels.add(target_order.client_order_id)
    full_fill = TestEventStubs.order_filled(target_order, instrument=env.instrument)
    target_order.apply(full_fill)  # update order state to FILLED (closed)

    # Act
    strategy.on_order_filled(full_fill)

    # Assert — fully filled order ID removed; no OrderCanceled will ever arrive for it
    assert target_order.client_order_id not in strategy._pending_self_cancels


def test_grid_tick_snap_no_float_boundary_drift(env):
    # Arrange — AUD/USD has price_precision=5, tick=0.00001. With mid=0.65 and
    # grid_step_bps=10, buy_raw = 0.65 * 0.999 = 0.6493499999... in f64.
    # The instrument's next_bid_price must snap this to 0.64935 (not 0.64934).
    strategy = _make_strategy(env, num_levels=1, grid_step_bps=10)
    strategy.start()
    mid = Price(0.65, 5)

    # Act
    grid = strategy._grid_orders(
        mid,
        net_position=0.0,
        worst_long=Decimal(0),
        worst_short=Decimal(0),
    )

    # Assert
    buy_price = next(float(p) for side, p in grid if side == OrderSide.BUY)
    assert buy_price == pytest.approx(0.64935)


def test_on_stop_cancels_all_orders(env):
    # Arrange
    strategy = _make_strategy(env, num_levels=3)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)

    # Act
    strategy.stop()
    env.exchange.process(env.clock.timestamp_ns())

    # Assert
    assert len(env.cache.orders_open()) == 0


def test_on_reset_clears_state(env):
    # Arrange
    strategy = _make_strategy(env)
    strategy.start()
    _process_quote(env, bid_price=0.64990, ask_price=0.65010)
    strategy.stop()

    # Act
    strategy.reset()

    # Assert
    assert strategy._instrument is None
    assert strategy._last_quoted_mid is None
    assert strategy._price_precision is None
    assert strategy._pending_self_cancels == set()

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

from __future__ import annotations

from decimal import Decimal

from strategies.backtest_surface import MarketDataAuditActor
from strategies.backtest_surface import MarketDataAuditActorConfig
from strategies.backtest_surface import RoutedOrderExecAlgorithm
from strategies.backtest_surface import RoutedOrderExecAlgorithmConfig
from strategies.backtest_surface import RoutedOrderProbe
from strategies.backtest_surface import RoutedOrderProbeConfig
from strategies.backtest_surface import StreamingWhipsaw
from strategies.backtest_surface import StreamingWhipsawConfig

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.execution import BestPriceFillModel
from nautilus_trader.execution import OneTickSlippageFillModel
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import ActorId
from nautilus_trader.model import AggregationSource
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderBookDepth10
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.trading import BookImbalanceActorConfig
from nautilus_trader.trading import CompositeMarketMakerConfig
from nautilus_trader.trading import EmaCrossConfig
from nautilus_trader.trading import ExecutionAlgorithmConfig
from nautilus_trader.trading import GridMarketMakerConfig
from nautilus_trader.trading import ImportableExecAlgorithmConfig
from nautilus_trader.trading import ImportableStrategyConfig
from tests.providers import TestInstrumentProvider


USD = Currency.from_str("USD")
USDT = Currency.from_str("USDT")


def test_native_grid_market_maker_requotes_from_python_surface():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        fill_model=BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
        latency_model=StaticLatencyModel(base_latency_nanos=1_000),
    )
    engine.add_instrument(instrument)
    engine.add_builtin_strategy(
        "GridMarketMaker",
        GridMarketMakerConfig(
            instrument_id=instrument.id,
            max_position=Quantity.from_str("10.00000"),
            trade_size=Quantity.from_str("0.10000"),
            num_levels=3,
            grid_step_bps=10,
            requote_threshold_bps=5,
        ),
    )

    engine.add_data(_crypto_quotes(instrument, count=20, mid_start=Decimal("2000.00")))
    engine.run()
    result = engine.get_result()

    assert result.iterations == 20
    assert result.total_orders >= 12
    assert result.summary["orders.open"] == "0"
    assert result.summary["orders.closed"] == result.summary["orders.total"]
    engine.dispose()


def test_native_composite_market_maker_reacts_to_signal_instrument():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    traded = TestInstrumentProvider.ethusdt_binance()
    signal = TestInstrumentProvider.btcusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        fill_model=BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
    )
    engine.add_instrument(traded)
    engine.add_instrument(signal)
    engine.add_builtin_strategy(
        "CompositeMarketMaker",
        CompositeMarketMakerConfig(
            instrument_id=traded.id,
            signal_instrument_id=signal.id,
            max_position=Quantity.from_str("5.00000"),
            trade_size=Quantity.from_str("0.05000"),
            half_spread_bps=4,
            signal_skew_factor=0.15,
            signal_baseline=Price.from_str("30000.00"),
            requote_threshold_bps=3,
        ),
    )

    data = []
    data.extend(
        _crypto_quotes(signal, count=14, mid_start=Decimal("30000.00"), mid_step=Decimal(8)),
    )
    data.extend(
        _crypto_quotes(traded, count=14, mid_start=Decimal("2000.00"), mid_step=Decimal("0.80")),
    )
    engine.add_data(data)
    engine.run()
    result = engine.get_result()

    assert result.iterations == 28
    assert result.total_orders >= 2
    assert result.summary["orders.open"] == "0"
    engine.dispose()


def test_native_ema_cross_trades_whipsaw_quote_data():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        fill_model=BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
    )
    engine.add_instrument(instrument)
    engine.add_builtin_strategy(
        "EmaCross",
        EmaCrossConfig(
            instrument_id=instrument.id,
            trade_size=Quantity.from_str("0.10000"),
            fast_period=3,
            slow_period=6,
        ),
    )

    engine.add_data(_crypto_whipsaw_quotes(instrument, count=30))
    engine.run()
    result = engine.get_result()

    assert result.iterations == 30
    assert result.total_orders >= 4
    assert result.total_positions >= 2
    assert result.summary["positions.open"] == "0"
    engine.dispose()


def test_builtin_book_imbalance_actor_consumes_quotes():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
    )
    engine.add_instrument(instrument)
    engine.add_builtin_actor(
        "BookImbalanceActor",
        BookImbalanceActorConfig(instrument_ids=[instrument.id], log_interval=0),
    )

    engine.add_data(_crypto_quotes(instrument, count=12, mid_start=Decimal("2000.00")))
    engine.run()
    result = engine.get_result()

    assert result.iterations == 12
    assert result.total_orders == 0
    assert result.summary["venues.total"] == "1"
    engine.dispose()


def test_importable_actor_receives_quotes_and_depth_snapshot_books():
    MarketDataAuditActor.reset_observations()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        book_type=BookType.L2_MBP,
    )
    engine.add_instrument(instrument)
    engine.add_actor_from_config(
        ImportableActorConfig(
            actor_path="strategies.backtest_surface:MarketDataAuditActor",
            config_path="strategies.backtest_surface:MarketDataAuditActorConfig",
            config={
                "instrument_id": str(instrument.id),
                "log_events": False,
            },
        ),
    )

    data = []
    data.extend(_crypto_quotes(instrument, count=4, mid_start=Decimal("2000.00")))
    data.extend(_book_depths(instrument, count=4))
    engine.add_data(data)
    engine.run()
    result = engine.get_result()

    assert result.iterations == 8
    assert result.total_orders == 0
    assert MarketDataAuditActor.quote_count == 4
    assert MarketDataAuditActor.book_count >= 1
    assert MarketDataAuditActor.last_bid == Price.from_str("2002.95")
    assert MarketDataAuditActor.last_book_bid == Price.from_str("2000.20")
    assert MarketDataAuditActor.last_book_ask == Price.from_str("2000.40")
    engine.dispose()


def test_importable_strategy_routes_synthetic_bars_through_native_twap():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.btcusdt_binance()
    bar_type = BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL")
    algo_id = ExecAlgorithmId("TWAP-SURFACE")
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[
            Money(10.0, Currency.from_str("BTC")),
            Money(10_000_000.0, USDT),
        ],
    )
    engine.add_instrument(instrument)
    engine.add_native_exec_algorithm(
        "TwapAlgorithm",
        ExecutionAlgorithmConfig(exec_algorithm_id=algo_id),
    )
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.ema_cross_twap:EMACrossTWAP",
            config_path="strategies.ema_cross_twap:EMACrossTWAPConfig",
            config={
                "instrument_id": str(instrument.id),
                "bar_type": str(bar_type),
                "trade_size": "0.010000",
                "fast_ema_period": 2,
                "slow_ema_period": 3,
                "exec_algorithm_id": str(algo_id),
                "twap_horizon_secs": 4.0,
                "twap_interval_secs": 1.0,
            },
        ),
    )

    closes = [
        Decimal("50000.00"),
        Decimal("49950.00"),
        Decimal("50080.00"),
        Decimal("50120.00"),
        Decimal("49800.00"),
        Decimal("49750.00"),
        Decimal("50200.00"),
        Decimal("50300.00"),
    ]
    engine.add_data(_btc_bars(instrument, bar_type, closes))
    engine.run()
    result = engine.get_result()
    orders = engine.cache.orders()
    primary_orders = [order for order in orders if order.exec_spawn_id is None]
    spawned_orders = [order for order in orders if order.exec_spawn_id is not None]

    assert result.iterations == len(closes)
    assert primary_orders
    assert spawned_orders
    assert all(order.exec_algorithm_id == algo_id for order in orders)
    assert all(order.status == OrderStatus.FILLED for order in orders)

    for primary in primary_orders:
        children = [
            order for order in spawned_orders if order.exec_spawn_id == primary.client_order_id
        ]
        sequence_qty = primary.quantity.as_decimal() + sum(
            order.quantity.as_decimal() for order in children
        )
        assert sequence_qty == Decimal("0.010000")
    engine.dispose()


def test_importable_strategy_routes_orders_through_importable_exec_algorithm():
    RoutedOrderExecAlgorithm.reset_observations()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    algo_id = ExecAlgorithmId("PY-ROUTE")
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
    )
    engine.add_instrument(instrument)
    engine.add_exec_algorithm_from_config(
        ImportableExecAlgorithmConfig(
            exec_algorithm_path="strategies.backtest_surface:RoutedOrderExecAlgorithm",
            config_path="strategies.backtest_surface:RoutedOrderExecAlgorithmConfig",
            config={
                "exec_algorithm_id": str(algo_id),
                "log_events": False,
                "log_commands": False,
            },
        ),
    )
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.backtest_surface:RoutedOrderProbe",
            config_path="strategies.backtest_surface:RoutedOrderProbeConfig",
            config={
                "instrument_id": str(instrument.id),
                "trade_size": "0.10000",
                "exec_algorithm_id": str(algo_id),
            },
        ),
    )

    engine.add_data(_crypto_quotes(instrument, count=3, mid_start=Decimal("2000.00")))
    engine.run()
    result = engine.get_result()
    orders = engine.cache.orders()

    assert result.iterations == 3
    assert result.total_orders == 1
    assert orders[0].exec_algorithm_id == algo_id
    assert orders[0].status == OrderStatus.INITIALIZED
    assert RoutedOrderExecAlgorithm.received_client_order_ids == [str(orders[0].client_order_id)]
    assert RoutedOrderExecAlgorithm.received_exec_algorithm_ids == [algo_id]
    assert RoutedOrderExecAlgorithm.signal_values == [str(orders[0].client_order_id)]
    engine.dispose()


def test_run_window_uses_inclusive_bounds_after_clear_data():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
    )
    engine.add_instrument(instrument)
    quotes = _crypto_quotes(instrument, count=10, mid_start=Decimal("2000.00"))
    engine.add_data(quotes[:5])
    engine.clear_data()
    engine.add_data(quotes)

    engine.run(
        start=quotes[2].ts_event,
        end=quotes[5].ts_event,
        run_config_id="windowed-replay",
    )
    result = engine.get_result()

    assert engine.run_config_id == "windowed-replay"
    assert engine.iteration == 4
    assert engine.backtest_start == quotes[2].ts_event
    assert engine.backtest_end == quotes[5].ts_event
    assert result.iterations == 4
    assert result.total_orders == 0
    engine.dispose()


def test_importable_strategy_processes_bars_trades_and_reference_data():
    instrument = TestInstrumentProvider.audusd_sim()
    bar_type = BarType(
        instrument.id,
        BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
        AggregationSource.EXTERNAL,
    )
    engine = _signal_harvest_engine(instrument, bar_type)

    engine.add_data(_reference_data(instrument))
    engine.add_data(_audusd_trades(instrument, count=6))
    engine.add_data(_audusd_bars(instrument, bar_type, count=14))
    engine.run()
    result = engine.get_result()

    assert result.iterations == 25
    assert result.total_orders >= 3
    assert result.total_positions >= 1
    assert result.summary["positions.open"] == "0"
    engine.dispose()


def test_importable_strategy_reruns_after_reset_and_report_generation():
    instrument = TestInstrumentProvider.audusd_sim()
    bar_type = BarType(
        instrument.id,
        BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
        AggregationSource.EXTERNAL,
    )
    engine = _signal_harvest_engine(instrument, bar_type)
    engine.add_data(_reference_data(instrument))
    engine.add_data(_audusd_trades(instrument, count=6))
    engine.add_data(_audusd_bars(instrument, bar_type, count=14))

    engine.run()
    first = engine.get_result()
    orders = engine.generate_orders_report()
    order_fills = engine.generate_order_fills_report()
    fills = engine.generate_fills_report()
    positions = engine.generate_positions_report()
    account = engine.generate_account_report(venue=Venue("SIM"))

    assert len(orders) == first.total_orders
    assert len(order_fills) >= 1
    assert len(fills) >= 1
    assert len(positions) >= 1
    assert len(account) >= 1

    engine.reset()
    engine.change_fill_model(
        Venue("SIM"),
        BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
    )
    engine.run()
    second = engine.get_result()

    assert second.iterations == first.iterations
    assert second.total_orders == first.total_orders
    assert second.total_positions == first.total_positions
    assert second.summary["positions.open"] == "0"
    engine.dispose()


def test_importable_strategy_runs_from_l2_book_deltas():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        book_type=BookType.L2_MBP,
        fill_model=BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
    )
    engine.add_instrument(instrument)
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.backtest_surface:BookChurn",
            config_path="strategies.backtest_surface:BookChurnConfig",
            config={
                "instrument_id": str(instrument.id),
                "trade_size": "0.10000",
            },
        ),
    )

    engine.add_data(_book_deltas(instrument))
    engine.run()
    result = engine.get_result()

    assert result.iterations == 5
    assert result.total_orders >= 3
    assert result.total_positions >= 1
    assert result.summary["orders.open"] == "0"
    engine.dispose()


def test_streaming_run_keeps_strategy_state_across_batches():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.audusd_sim()
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USD)],
        base_currency=USD,
        fill_model=BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
    )
    engine.add_instrument(instrument)
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.backtest_surface:StreamingWhipsaw",
            config_path="strategies.backtest_surface:StreamingWhipsawConfig",
            config={
                "instrument_id": str(instrument.id),
                "trade_size": "100000",
            },
        ),
    )

    quotes = _audusd_quotes(instrument, count=10)
    engine.add_data(quotes[:5])
    engine.run(streaming=True)
    engine.clear_data()
    engine.add_data(quotes[5:])
    engine.run(streaming=False)
    result = engine.get_result()

    assert result.iterations == 10
    assert result.total_orders == 4
    assert result.total_positions == 2
    assert result.summary["positions.open"] == "0"
    engine.dispose()


def test_strategy_instance_retains_config_object():
    instrument = TestInstrumentProvider.audusd_sim()
    config = StreamingWhipsawConfig(instrument_id=str(instrument.id), trade_size="100000")

    strategy = StreamingWhipsaw(config)

    assert strategy.config is config
    assert strategy.config.instrument_id == str(instrument.id)
    assert strategy.config.trade_size == "100000"


def test_actor_instance_retains_config_object():
    instrument = TestInstrumentProvider.ethusdt_binance()
    config = MarketDataAuditActorConfig(instrument_id=str(instrument.id), log_events=False)

    actor = MarketDataAuditActor(config)

    assert actor.config is config
    assert actor.config.instrument_id == str(instrument.id)


def test_add_actor_with_constructed_instance_consumes_quotes():
    MarketDataAuditActor.reset_observations()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        book_type=BookType.L2_MBP,
    )
    engine.add_instrument(instrument)

    config = MarketDataAuditActorConfig(instrument_id=str(instrument.id), log_events=False)
    actor = MarketDataAuditActor(config)
    engine.add_actor(actor)

    data = []
    data.extend(_crypto_quotes(instrument, count=4, mid_start=Decimal("2000.00")))
    data.extend(_book_depths(instrument, count=4))
    engine.add_data(data)
    engine.run()
    result = engine.get_result()

    assert result.iterations == 8
    assert result.total_orders == 0
    assert MarketDataAuditActor.quote_count == 4
    assert MarketDataAuditActor.book_count >= 1
    engine.dispose()


def test_add_strategy_with_constructed_instance_submits_orders():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.audusd_sim()
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USD)],
        base_currency=USD,
        fill_model=BestPriceFillModel(prob_fill_on_limit=1.0, prob_slippage=0.0),
    )
    engine.add_instrument(instrument)

    config = StreamingWhipsawConfig(instrument_id=str(instrument.id), trade_size="100000")
    strategy = StreamingWhipsaw(config)
    engine.add_strategy(strategy)

    engine.add_data(_audusd_quotes(instrument, count=10))
    engine.run()
    result = engine.get_result()

    assert result.iterations == 10
    assert result.total_orders == 4
    assert result.total_positions == 2
    engine.dispose()


def test_add_exec_algorithm_and_strategy_instances_route_orders():
    RoutedOrderExecAlgorithm.reset_observations()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    algo_id = ExecAlgorithmId("PY-ROUTE-INSTANCE")
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
    )
    engine.add_instrument(instrument)

    algo = RoutedOrderExecAlgorithm(
        RoutedOrderExecAlgorithmConfig(
            exec_algorithm_id=str(algo_id),
            log_events=False,
            log_commands=False,
        ),
    )
    engine.add_exec_algorithm(algo)

    probe = RoutedOrderProbe(
        RoutedOrderProbeConfig(
            instrument_id=str(instrument.id),
            trade_size="0.10000",
            exec_algorithm_id=str(algo_id),
        ),
    )
    engine.add_strategy(probe)

    engine.add_data(_crypto_quotes(instrument, count=3, mid_start=Decimal("2000.00")))
    engine.run()
    result = engine.get_result()
    orders = engine.cache.orders()

    assert result.iterations == 3
    assert result.total_orders == 1
    assert orders[0].exec_algorithm_id == algo_id
    assert RoutedOrderExecAlgorithm.received_exec_algorithm_ids == [algo_id]
    engine.dispose()


def test_add_actors_registers_multiple_constructed_instances():
    MarketDataAuditActor.reset_observations()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
        book_type=BookType.L2_MBP,
    )
    engine.add_instrument(instrument)

    engine.add_actors(
        [
            MarketDataAuditActor(
                MarketDataAuditActorConfig(
                    instrument_id=str(instrument.id),
                    actor_id=ActorId("AUDIT-A"),
                    log_events=False,
                ),
            ),
            MarketDataAuditActor(
                MarketDataAuditActorConfig(
                    instrument_id=str(instrument.id),
                    actor_id=ActorId("AUDIT-B"),
                    log_events=False,
                ),
            ),
        ],
    )

    data = []
    data.extend(_crypto_quotes(instrument, count=4, mid_start=Decimal("2000.00")))
    data.extend(_book_depths(instrument, count=4))
    engine.add_data(data)
    engine.run()
    result = engine.get_result()

    assert result.iterations == 8
    # Both registered actors count all 4 quotes (4 x 2)
    assert MarketDataAuditActor.quote_count == 8
    engine.dispose()


def _signal_harvest_engine(instrument, bar_type: BarType) -> BacktestEngine:
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USD)],
        base_currency=USD,
        fill_model=OneTickSlippageFillModel(prob_fill_on_limit=1.0, prob_slippage=1.0),
    )
    engine.add_instrument(instrument)
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.backtest_surface:SignalHarvest",
            config_path="strategies.backtest_surface:SignalHarvestConfig",
            config={
                "instrument_id": str(instrument.id),
                "bar_type": str(bar_type),
                "trade_size": "100000",
            },
        ),
    )
    return engine


def _crypto_quotes(
    instrument,
    count: int,
    mid_start: Decimal,
    mid_step: Decimal = Decimal("1.00"),
) -> list[QuoteTick]:
    base_ns = 1_600_000_000_000_000_000
    quotes = []

    for i in range(count):
        mid = mid_start + (mid_step * i)
        quotes.append(
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_decimal_dp(mid - Decimal("0.05"), instrument.price_precision),
                ask_price=Price.from_decimal_dp(mid + Decimal("0.05"), instrument.price_precision),
                bid_size=Quantity.from_decimal_dp(Decimal(10), instrument.size_precision),
                ask_size=Quantity.from_decimal_dp(Decimal(10), instrument.size_precision),
                ts_event=base_ns + (i * 1_000_000_000),
                ts_init=base_ns + (i * 1_000_000_000),
            ),
        )
    return quotes


def _audusd_quotes(instrument, count: int) -> list[QuoteTick]:
    base_ns = 1_600_000_100_000_000_000
    quotes = []

    for i in range(count):
        bid = Decimal("0.70000") + (Decimal(i % 4) * Decimal("0.00010"))
        quotes.append(
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_decimal_dp(bid, instrument.price_precision),
                ask_price=Price.from_decimal_dp(
                    bid + Decimal("0.00020"),
                    instrument.price_precision,
                ),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=base_ns + (i * 1_000_000_000),
                ts_init=base_ns + (i * 1_000_000_000),
            ),
        )
    return quotes


def _crypto_whipsaw_quotes(instrument, count: int) -> list[QuoteTick]:
    base_ns = 1_600_000_200_000_000_000
    quotes = []

    for i in range(count):
        mid = Decimal("2000.00") + (Decimal((i % 10) - 5) * Decimal(2))
        quotes.append(
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_decimal_dp(mid - Decimal("0.05"), instrument.price_precision),
                ask_price=Price.from_decimal_dp(mid + Decimal("0.05"), instrument.price_precision),
                bid_size=Quantity.from_decimal_dp(10, instrument.size_precision),
                ask_size=Quantity.from_decimal_dp(10, instrument.size_precision),
                ts_event=base_ns + (i * 1_000_000_000),
                ts_init=base_ns + (i * 1_000_000_000),
            ),
        )
    return quotes


def _audusd_bars(instrument, bar_type: BarType, count: int) -> list[Bar]:
    base_ns = 1_600_000_010_000_000_000
    bars = []

    for i in range(count):
        close = Decimal("0.70000") + (Decimal(i) * Decimal("0.00008"))
        bars.append(
            Bar(
                bar_type=bar_type,
                open=Price.from_decimal_dp(close - Decimal("0.00003"), instrument.price_precision),
                high=Price.from_decimal_dp(close + Decimal("0.00020"), instrument.price_precision),
                low=Price.from_decimal_dp(close - Decimal("0.00012"), instrument.price_precision),
                close=Price.from_decimal_dp(close, instrument.price_precision),
                volume=Quantity.from_int(1_000_000),
                ts_event=base_ns + (i * 60_000_000_000),
                ts_init=base_ns + (i * 60_000_000_000),
            ),
        )
    return bars


def _btc_bars(instrument, bar_type: BarType, closes: list[Decimal]) -> list[Bar]:
    base_ns = 1_600_000_020_000_000_000
    bars = []

    for i, close in enumerate(closes):
        bars.append(
            Bar(
                bar_type=bar_type,
                open=Price.from_decimal_dp(close - Decimal("10.00"), instrument.price_precision),
                high=Price.from_decimal_dp(close + Decimal("40.00"), instrument.price_precision),
                low=Price.from_decimal_dp(close - Decimal("40.00"), instrument.price_precision),
                close=Price.from_decimal_dp(close, instrument.price_precision),
                volume=Quantity.from_decimal_dp(Decimal("5.000000"), instrument.size_precision),
                ts_event=base_ns + (i * 60_000_000_000),
                ts_init=base_ns + (i * 60_000_000_000),
            ),
        )
    return bars


def _audusd_trades(instrument, count: int) -> list[TradeTick]:
    base_ns = 1_600_000_005_000_000_000
    trades = []

    for i in range(count):
        price = Decimal("0.70000") + (Decimal(i) * Decimal("0.00005"))
        trades.append(
            TradeTick(
                instrument_id=instrument.id,
                price=Price.from_decimal_dp(price, instrument.price_precision),
                size=Quantity.from_int(100_000),
                aggressor_side=AggressorSide.BUYER if i % 2 == 0 else AggressorSide.SELLER,
                trade_id=TradeId(f"T-{i}"),
                ts_event=base_ns + (i * 1_000_000_000),
                ts_init=base_ns + (i * 1_000_000_000),
            ),
        )
    return trades


def _reference_data(instrument) -> list:
    base_ns = 1_600_000_000_000_000_000
    price = Price.from_str("0.70000")
    return [
        InstrumentStatus(instrument.id, MarketStatusAction.TRADING, base_ns, base_ns),
        MarkPriceUpdate(instrument.id, price, base_ns + 1, base_ns + 1),
        IndexPriceUpdate(instrument.id, price, base_ns + 2, base_ns + 2),
        FundingRateUpdate(instrument.id, Decimal("0.0001"), base_ns + 3, base_ns + 3),
        InstrumentClose(
            instrument_id=instrument.id,
            close_price=price,
            close_type=InstrumentCloseType.END_OF_SESSION,
            ts_event=base_ns + 4,
            ts_init=base_ns + 4,
        ),
    ]


def _book_deltas(instrument) -> list[OrderBookDeltas]:
    base_ns = 1_600_000_000_000_000_000
    batches = []

    for i in range(5):
        bid = Decimal("1999.90") + (Decimal(i) * Decimal("0.10"))
        ask = Decimal("2000.10") + (Decimal(i) * Decimal("0.10"))
        ts = base_ns + (i * 1_000_000_000)
        batches.append(
            OrderBookDeltas(
                instrument_id=instrument.id,
                deltas=[
                    OrderBookDelta(
                        instrument.id,
                        BookAction.ADD if i == 0 else BookAction.UPDATE,
                        BookOrder(
                            OrderSide.BUY,
                            Price.from_decimal_dp(bid, instrument.price_precision),
                            Quantity.from_decimal_dp(Decimal(10), instrument.size_precision),
                            1,
                        ),
                        0,
                        (i * 2) + 1,
                        ts,
                        ts,
                    ),
                    OrderBookDelta(
                        instrument.id,
                        BookAction.ADD if i == 0 else BookAction.UPDATE,
                        BookOrder(
                            OrderSide.SELL,
                            Price.from_decimal_dp(ask, instrument.price_precision),
                            Quantity.from_decimal_dp(Decimal(10), instrument.size_precision),
                            2,
                        ),
                        0,
                        (i * 2) + 2,
                        ts,
                        ts,
                    ),
                ],
            ),
        )
    return batches


def _book_depths(instrument, count: int) -> list[OrderBookDepth10]:
    base_ns = 1_600_000_150_000_000_000
    depths = []

    for i in range(count):
        top_bid = Decimal("1999.90") + (Decimal(i) * Decimal("0.10"))
        top_ask = Decimal("2000.10") + (Decimal(i) * Decimal("0.10"))
        bids = []
        asks = []

        for level in range(10):
            offset = Decimal(level) * Decimal("0.01")
            size = Decimal(10 + level)
            bids.append(
                BookOrder(
                    OrderSide.BUY,
                    Price.from_decimal_dp(top_bid - offset, instrument.price_precision),
                    Quantity.from_decimal_dp(size, instrument.size_precision),
                    level + 1,
                ),
            )
            asks.append(
                BookOrder(
                    OrderSide.SELL,
                    Price.from_decimal_dp(top_ask + offset, instrument.price_precision),
                    Quantity.from_decimal_dp(size, instrument.size_precision),
                    level + 11,
                ),
            )

        ts = base_ns + (i * 1_000_000_000)
        depths.append(
            OrderBookDepth10(
                instrument_id=instrument.id,
                bids=bids,
                asks=asks,
                bid_counts=[1] * 10,
                ask_counts=[1] * 10,
                flags=0,
                sequence=i + 1,
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return depths

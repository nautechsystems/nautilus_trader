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
Test backtest node behavior.
"""

from __future__ import annotations

from decimal import Decimal
from pathlib import Path

import pytest
from strategies import backtest_surface
from strategies.backtest_surface import QuoteCountActor
from strategies.backtest_surface import QuoteCountActorConfig
from strategies.backtest_surface import RoutedOrderExecutionAlgorithm
from strategies.backtest_surface import RoutedOrderExecutionAlgorithmConfig
from strategies.backtest_surface import StreamingWhipsaw
from strategies.backtest_surface import StreamingWhipsawConfig

from nautilus_trader.analysis import TearsheetConfig
from nautilus_trader.analysis import TearsheetStatsTableChart
from nautilus_trader.analysis import create_tearsheet
from nautilus_trader.analysis import tearsheet
from nautilus_trader.analysis.reporter import ReportProvider
from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.core import UUID4
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StandardMarginModel
from nautilus_trader.model import Venue
from nautilus_trader.persistence import DataCatalogConfig
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import StreamingConfig
from nautilus_trader.trading import EmaCrossConfig
from nautilus_trader.trading import ImportableExecutionAlgorithmConfig
from nautilus_trader.trading import ImportableStrategyConfig
from tests.providers import TestInstrumentProvider
from tests.stubs import TestDataProviderPyo3


def test_node_construction() -> None:
    """
    Test node construction.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    assert node is not None


def test_node_installs_configured_margin_model() -> None:
    """
    Test node installs the configured margin model on the account.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
        base_currency=Currency.from_str("USD"),
        default_leverage=Decimal(10),
        margin_model=StandardMarginModel(),
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    node = BacktestNode([config])

    try:
        assert len(node.run()) == 1
        account = node.get_engine_cache(config.id).account_for_venue(Venue("SIM"))

        assert account is not None
        assert account.calculate_initial_margin(
            instrument=instrument,
            quantity=Quantity.from_int(10_000),
            price=Price.from_str("0.80000"),
        ) == Money.from_str("240.00 USD")
    finally:
        node.dispose()


def test_node_uses_margin_account_default_leverage() -> None:
    """
    Test node uses the low-level margin account leverage default.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
        base_currency=Currency.from_str("USD"),
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    node = BacktestNode([config])

    try:
        assert len(node.run()) == 1
        account = node.get_engine_cache(config.id).account_for_venue(Venue("SIM"))

        assert account.default_leverage == Decimal(10)
    finally:
        node.dispose()


def test_node_applies_configured_latency_model(tmp_path: Path) -> None:
    """
    Test node applies configured latency during execution.
    """
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    quotes = _whipsaw_quotes(instrument, count=10)
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks(quotes)
    venue = BacktestVenueConfig(
        name="BINANCE",
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USDT"],
        latency_model=StaticLatencyModel(base_latency_nanos=1_000_000_000),
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path=str(catalog_path),
        instrument_id=instrument.id,
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[data],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
    )
    node = BacktestNode([config])

    try:
        node.build()
        node.add_strategy_from_config(
            config.id,
            ImportableStrategyConfig(
                strategy_path="tests.strategies.backtest_surface:StreamingWhipsaw",
                config_path="tests.strategies.backtest_surface:StreamingWhipsawConfig",
                config={
                    "instrument_id": str(instrument.id),
                    "trade_size": "1.00000",
                },
            ),
        )
        result = node.run()[0]

        assert result.backtest_end == quotes[-1].ts_event + 1_000_000_000
    finally:
        node.dispose()


@pytest.mark.parametrize("from_config", [False, True])
def test_node_registers_actor_forms(tmp_path: Path, from_config: bool) -> None:
    """
    Test node actor registration forms receive runtime market data.
    """
    QuoteCountActor.reset_observations()
    node, config, instrument, quotes = _build_component_node(tmp_path, quote_count=4)

    try:
        if from_config:
            node.add_actor_from_config(
                config.id,
                ImportableActorConfig(
                    actor_path="strategies.backtest_surface:QuoteCountActor",
                    config_path="strategies.backtest_surface:QuoteCountActorConfig",
                    config={"instrument_id": str(instrument.id)},
                ),
            )
        else:
            node.add_actor(
                config.id,
                QuoteCountActor(QuoteCountActorConfig(instrument_id=str(instrument.id))),
            )

        node.run()

        assert QuoteCountActor.quote_count == len(quotes)
        assert QuoteCountActor.last_bid == quotes[-1].bid_price
    finally:
        node.dispose()


@pytest.mark.parametrize("from_config", [False, True])
def test_node_registers_strategy_forms(tmp_path: Path, from_config: bool) -> None:
    """
    Test node strategy registration forms submit orders at runtime.
    """
    node, config, instrument, _ = _build_component_node(tmp_path, quote_count=10)

    try:
        if from_config:
            node.add_strategy_from_config(
                config.id,
                ImportableStrategyConfig(
                    strategy_path="strategies.backtest_surface:StreamingWhipsaw",
                    config_path="strategies.backtest_surface:StreamingWhipsawConfig",
                    config={
                        "instrument_id": str(instrument.id),
                        "trade_size": "1.00000",
                    },
                ),
            )
        else:
            node.add_strategy(
                config.id,
                StreamingWhipsaw(
                    StreamingWhipsawConfig(
                        instrument_id=str(instrument.id),
                        trade_size="1.00000",
                    ),
                ),
            )

        result = node.run()[0]

        assert result.total_orders == 4
        assert result.total_positions == 2
    finally:
        node.dispose()


@pytest.mark.parametrize("from_config", [False, True])
def test_node_registers_execution_algorithm_forms(tmp_path: Path, from_config: bool) -> None:
    """
    Test node execution-algorithm registration forms receive routed orders.
    """
    RoutedOrderExecutionAlgorithm.reset_observations()
    node, config, instrument, _ = _build_component_node(tmp_path, quote_count=3)
    algorithm_id = ExecAlgorithmId("NODE-EXEC")

    try:
        if from_config:
            node.add_exec_algorithm_from_config(
                config.id,
                ImportableExecutionAlgorithmConfig(
                    exec_algorithm_path="strategies.backtest_surface:RoutedOrderExecutionAlgorithm",
                    config_path="strategies.backtest_surface:RoutedOrderExecutionAlgorithmConfig",
                    config={
                        "exec_algorithm_id": str(algorithm_id),
                        "log_events": False,
                        "log_commands": False,
                    },
                ),
            )
        else:
            node.add_exec_algorithm(
                config.id,
                RoutedOrderExecutionAlgorithm(
                    RoutedOrderExecutionAlgorithmConfig(
                        exec_algorithm_id=str(algorithm_id),
                        log_events=False,
                        log_commands=False,
                    ),
                ),
            )
        node.add_strategy_from_config(
            config.id,
            ImportableStrategyConfig(
                strategy_path="strategies.backtest_surface:RoutedOrderProbe",
                config_path="strategies.backtest_surface:RoutedOrderProbeConfig",
                config={
                    "instrument_id": str(instrument.id),
                    "trade_size": "0.10000",
                    "exec_algorithm_id": str(algorithm_id),
                },
            ),
        )

        result = node.run()[0]

        assert result.total_orders == 1
        assert RoutedOrderExecutionAlgorithm.received_exec_algorithm_ids == [algorithm_id]
        assert RoutedOrderExecutionAlgorithm.cache_instrument_ids == [str(instrument.id)]
    finally:
        node.dispose()


def test_node_rejects_disposed_execution_algorithm_before_construction(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test node rejects a disposed execution algorithm before construction.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
    )
    node = BacktestNode([config])
    constructor_calls: list[bool] = []

    class ConstructorProbe:
        def __init__(self) -> None:
            constructor_calls.append(True)

    monkeypatch.setattr(backtest_surface, "ConstructorProbe", ConstructorProbe, raising=False)

    try:
        assert len(node.run()) == 1

        with pytest.raises(RuntimeError, match="Cannot add components to disposed trader"):
            node.add_exec_algorithm_from_config(
                config.id,
                ImportableExecutionAlgorithmConfig(
                    exec_algorithm_path="strategies.backtest_surface:ConstructorProbe",
                    config_path="",
                    config={},
                ),
            )

        assert constructor_calls == []
    finally:
        node.dispose()


@pytest.mark.parametrize("replace_existing", [False, True])
def test_node_streams_output_to_new_or_replaced_directory(
    tmp_path: Path,
    replace_existing: bool,
) -> None:
    """
    Test configured streaming creates its run directory and replaces existing output.
    """
    instance_id = UUID4()
    output_path = tmp_path / "output"
    run_path = output_path / "backtest" / str(instance_id)
    sentinel = run_path / "stale.feather"
    if replace_existing:
        run_path.mkdir(parents=True)
        sentinel.write_bytes(b"stale")
    node, _config, _, _ = _build_component_node(
        tmp_path,
        quote_count=3,
        engine=BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
            instance_id=instance_id,
            streaming=StreamingConfig(
                catalog_path=str(output_path),
                fs_protocol="file",
                flush_interval_ms=1,
                replace_existing=replace_existing,
                rotation_mode="SIZE",
                max_file_size=1,
            ),
        ),
    )

    try:
        assert run_path.is_dir()
        assert sentinel.exists() is False

        result = node.run()[0]
        files = list((output_path / "backtest" / str(result.instance_id)).rglob("*.feather"))
        quote_files = [path for path in files if "quotes" in str(path)]

        assert files
        assert len(quote_files) == 3
    finally:
        node.dispose()


def test_node_builds_with_configured_catalog(tmp_path: Path) -> None:
    """
    Test node construction registers an existing built-in catalog.
    """
    input_path = tmp_path / "input"
    engine = BacktestEngineConfig(
        bypass_logging=True,
        run_analysis=False,
        catalogs=[DataCatalogConfig(path=str(input_path), name="history")],
    )
    node, _, _, quotes = _build_component_node(
        tmp_path,
        quote_count=4,
        engine=engine,
    )

    try:
        assert node.run()[0].iterations == len(quotes)
    finally:
        node.dispose()


def test_node_rejects_invalid_configured_catalog_path(tmp_path: Path) -> None:
    """
    Test node construction consumes and validates configured catalogs.
    """
    engine = BacktestEngineConfig(
        bypass_logging=True,
        run_analysis=False,
        catalogs=[DataCatalogConfig(path="\0")],
    )

    with pytest.raises(RuntimeError, match="Failed to create data catalog"):
        _build_component_node(
            tmp_path,
            quote_count=1,
            engine=engine,
            raise_exception=True,
        )


def test_node_rejects_duplicate_configured_catalog_names(tmp_path: Path) -> None:
    """
    Test node construction rejects duplicate catalog names without panicking.
    """
    catalog = DataCatalogConfig(path=str(tmp_path), name="history")
    engine = BacktestEngineConfig(
        bypass_logging=True,
        run_analysis=False,
        catalogs=[catalog, catalog],
    )

    with pytest.raises(RuntimeError, match="Duplicate data catalog name 'history'"):
        _build_component_node(
            tmp_path,
            quote_count=1,
            engine=engine,
            raise_exception=True,
        )


def test_node_exposes_builtin_strategy_registration() -> None:
    """
    Test node exposes builtin strategy registration.
    """
    assert hasattr(BacktestNode, "add_builtin_strategy")


def test_node_empty_configs_raises() -> None:
    """
    Test node empty configs raises.
    """
    with pytest.raises(RuntimeError, match="At least one run config"):
        BacktestNode([])


def test_node_venue_mismatch_raises() -> None:
    """
    Test node venue mismatch raises.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("BTC/USDT.BINANCE"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    with pytest.raises(RuntimeError, match="No venue config found for venue"):
        BacktestNode([config])


def test_node_repr() -> None:
    """
    Test node repr.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    assert "BacktestNode" in repr(node)


def test_node_dispose() -> None:
    """
    Test node dispose.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    node.dispose()


@pytest.mark.parametrize(
    ("method_name", "args"),
    [
        ("get_engine_cache", ()),
        ("get_engine_portfolio", ()),
        ("generate_orders_report", ()),
        ("generate_order_fills_report", ()),
        ("generate_fills_report", ()),
        ("generate_positions_report", ()),
        ("generate_account_report", (Venue("SIM"),)),
    ],
)
def test_node_post_run_inspection_unknown_config_raises(
    method_name: str,
    args: tuple[object, ...],
) -> None:
    """
    Test node post run inspection unknown config raises.
    """
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    config = BacktestRunConfig(venues=[venue], data=[])
    node = BacktestNode([config])

    with pytest.raises(RuntimeError, match="No engine for run config 'missing'"):
        getattr(node, method_name)("missing", *args)


def test_node_post_run_inspection_retains_exact_engine_state(tmp_path: Path) -> None:
    """
    Test node post run inspection retains exact engine state.
    """
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    quotes = _whipsaw_quotes(instrument, count=30)
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks(quotes)
    node, config = _build_ema_cross_node(
        str(catalog_path),
        instrument,
        chunk_size=7,
        dispose_on_completion=False,
    )

    try:
        result = node.run()[0]
        cache = node.get_engine_cache(config.id)
        portfolio = node.get_engine_portfolio(config.id)
        statistics = portfolio.statistics()
        account = cache.account_for_venue(Venue("BINANCE"))
        orders = cache.orders()
        positions = cache.positions()
        position_snapshots = cache.position_snapshots()

        orders_report = node.generate_orders_report(config.id)
        order_fills_report = node.generate_order_fills_report(config.id)
        fills_report = node.generate_fills_report(config.id)
        positions_report = node.generate_positions_report(config.id)
        account_report = node.generate_account_report(config.id, venue=Venue("BINANCE"))
        account_report_by_id = node.generate_account_report(config.id, account_id=account.id)

        with pytest.raises(ValueError, match="At least one of 'venue' or 'account_id'"):
            node.generate_account_report(config.id)

        assert cache.instrument_ids() == [instrument.id]
        assert cache.orders_total_count() == result.total_orders
        assert portfolio.account(venue=Venue("BINANCE")).id == account.id
        assert statistics.pnls.keys() == result.stats_pnls.keys()
        for currency in statistics.pnls:
            assert statistics.pnls[currency] == pytest.approx(
                result.stats_pnls[currency],
                nan_ok=True,
            )
        assert statistics.returns == pytest.approx(result.stats_returns, nan_ok=True)
        assert statistics.general == pytest.approx(result.stats_general, nan_ok=True)
        assert orders_report.equals(ReportProvider.generate_orders_report(orders))
        assert order_fills_report.equals(ReportProvider.generate_order_fills_report(orders))
        assert fills_report.equals(ReportProvider.generate_fills_report(orders))
        assert positions_report.equals(
            ReportProvider.generate_positions_report(positions, position_snapshots),
        )
        assert account_report.equals(ReportProvider.generate_account_report(account))
        assert account_report_by_id.equals(account_report)
    finally:
        node.dispose()


def test_result_tearsheet_rejects_default_disposed_node_state(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test result tearsheet rejects default disposed node state.
    """
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks(_whipsaw_quotes(instrument, count=30))
    node, config = _build_ema_cross_node(str(catalog_path), instrument, chunk_size=7)

    try:
        result = node.run()[0]
        run_configs = node.configs

        monkeypatch.setattr(tearsheet, "PLOTLY_AVAILABLE", True)

        assert [run_config.id for run_config in run_configs] == [config.id]
        assert run_configs[0].dispose_on_completion is True
        with pytest.raises(ValueError, match="dispose_on_completion=False"):
            create_tearsheet(
                result,
                node=node,
                output_path=None,
                config=TearsheetConfig(charts=[TearsheetStatsTableChart()]),
            )
    finally:
        node.dispose()


def test_node_streaming_matches_oneshot_from_local_catalog(tmp_path: Path) -> None:
    """
    Test node streaming matches oneshot from local catalog.
    """
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    quotes = _whipsaw_quotes(instrument, count=30)
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks(quotes)

    oneshot = _run_ema_cross_node(str(catalog_path), instrument, chunk_size=None)
    streaming = _run_ema_cross_node(str(catalog_path), instrument, chunk_size=7)

    assert oneshot.iterations == streaming.iterations == len(quotes)
    assert oneshot.total_events == streaming.total_events
    assert oneshot.total_orders == streaming.total_orders
    assert oneshot.total_positions == streaming.total_positions
    assert oneshot.total_orders >= 4
    assert oneshot.total_positions >= 2
    assert oneshot.summary["orders.open"] == streaming.summary["orders.open"] == "0"
    assert oneshot.summary["orders.closed"] == streaming.summary["orders.closed"]
    assert oneshot.summary["positions.open"] == streaming.summary["positions.open"] == "0"
    assert oneshot.summary["positions.closed"] == streaming.summary["positions.closed"]


def _run_ema_cross_node(catalog_path: object, instrument: object, chunk_size: object) -> object:
    node, _ = _build_ema_cross_node(catalog_path, instrument, chunk_size)
    result = node.run()[0]
    node.dispose()
    return result


def _build_component_node(
    tmp_path: Path,
    quote_count: int,
    engine: BacktestEngineConfig | None = None,
    raise_exception: bool = False,
) -> tuple[BacktestNode, BacktestRunConfig, object, list[object]]:
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog_path = tmp_path / "input"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    quotes = _whipsaw_quotes(instrument, count=quote_count)
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks(quotes)
    venue = BacktestVenueConfig(
        name="BINANCE",
        oms_type="NETTING",
        account_type="MARGIN",
        starting_balances=["1_000_000 USDT"],
        book_type="L1_MBP",
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path=str(catalog_path),
        instrument_id=instrument.id,
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[data],
        engine=engine or BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
        raise_exception=raise_exception,
    )
    node = BacktestNode([config])
    node.build()
    return node, config, instrument, quotes


def _build_ema_cross_node(
    catalog_path: object,
    instrument: object,
    chunk_size: object,
    dispose_on_completion: object = True,
) -> object:
    venue = BacktestVenueConfig(
        name="BINANCE",
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USDT"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path=catalog_path,
        instrument_id=instrument.id,
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[data],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        chunk_size=chunk_size,
        dispose_on_completion=dispose_on_completion,
    )
    node = BacktestNode([config])
    node.build()
    node.add_builtin_strategy(
        config.id,
        "EmaCross",
        EmaCrossConfig(
            instrument_id=instrument.id,
            trade_size=Quantity.from_str("0.10000"),
            fast_period=3,
            slow_period=6,
        ),
    )
    return node, config


def _whipsaw_quotes(instrument: object, count: object) -> object:
    base_ns = 1_600_000_200_000_000_000
    quotes = []

    for i in range(count):
        mid = Decimal("2000.00") + (Decimal((i % 10) - 5) * Decimal(2))
        quotes.append(
            TestDataProviderPyo3.quote_tick(
                instrument_id=instrument.id,
                bid_price=mid - Decimal("0.05"),
                ask_price=mid + Decimal("0.05"),
                bid_size="10.00000",
                ask_size="10.00000",
                ts_event=base_ns + (i * 1_000_000_000),
                ts_init=base_ns + (i * 1_000_000_000),
            ),
        )

    return quotes

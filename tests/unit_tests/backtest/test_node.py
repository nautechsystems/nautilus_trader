# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import multiprocessing
import tracemalloc
from unittest.mock import patch

import msgspec
import pytest

import nautilus_trader.backtest.node as node
from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.mocks.data import load_catalog_with_stub_quote_ticks_audusd
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.providers import get_test_data_large_path


class DummyStreamingSession:
    def __init__(self, chunk_size=None):
        self.chunk_size = chunk_size

    def to_query_result(self):
        return []


class DummyStreamingCatalog:
    def __init__(self, path: str, protocol: str | None):
        self.path = path
        self.fs_protocol = protocol
        self.calls = 0

    def get_file_list_from_data_cls(self, data_cls: type):
        self.calls += 1
        return [f"{self.path}/{data_cls.__name__}.parquet"]

    def filter_files(
        self,
        data_cls: type,
        file_paths: list[str],
        identifiers=None,
        start=None,
        end=None,
    ):
        return file_paths

    def backend_session(self, data_cls, identifiers, start, end, session, files):
        return session


class DummyStreamingEngine:
    def add_data(self, data, validate=True, sort=True):
        return None

    def run(self, start=None, end=None, run_config_id=None, streaming=None):
        return None

    def clear_data(self):
        return None

    def end(self):
        return None


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
_BTCUSDT_HUOBI = TestInstrumentProvider.btcusdt_future_binance()  # Use as stand-in for Huobi


def load_catalog_with_quote_ticks(
    catalog: ParquetDataCatalog,
    count: int | None = None,
) -> tuple[int, int]:
    """
    Load quote ticks to catalog, optionally limiting count.

    Returns tuple of (start_time_ns, end_time_ns) for the loaded data.

    """
    wrangler = QuoteTickDataWrangler(_AUDUSD_SIM)
    ticks = wrangler.process(TestDataProvider().read_csv_ticks("truefx/audusd-ticks.csv"))
    ticks.sort(key=lambda x: x.ts_init)
    if count is not None:
        ticks = ticks[:count]

    catalog.write_data([_AUDUSD_SIM])
    catalog.write_data(ticks)
    return ticks[0].ts_init, ticks[-1].ts_init


def load_catalog_with_large_tardis_quotes(
    catalog: ParquetDataCatalog,
    limit: int = 500_000,
) -> tuple[int, int]:
    """
    Load large Tardis quote tick data to catalog for memory testing.

    Uses the gzipped Huobi BTC-USD quotes file (~1.15M records) from
    tests/test_data/large/. This provides realistic market data for testing streaming
    memory behavior.

    Returns tuple of (start_time_ns, end_time_ns) for the loaded data.

    """
    filepath = get_test_data_large_path() / "tardis_huobi-dm-swap_quotes_2020-05-01_BTC-USD.csv.gz"
    if not filepath.exists():
        pytest.skip(f"Large test data not found: {filepath}")

    loader = TardisCSVDataLoader(instrument_id=_BTCUSDT_HUOBI.id)
    ticks = loader.load_quotes(filepath, limit=limit)

    catalog.write_data([_BTCUSDT_HUOBI])
    catalog.write_data(ticks)
    return ticks[0].ts_init, ticks[-1].ts_init


def _run_backtest_measure_memory(config_json: bytes, result_queue: multiprocessing.Queue) -> None:
    """
    Run backtest in subprocess and measure peak memory.

    Must be at module level for multiprocessing to pickle it.

    """
    tracemalloc.start()
    config = BacktestRunConfig.parse(config_json)
    node = BacktestNode(configs=[config])
    node.run()
    _, peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    result_queue.put(peak)


class TestBacktestNode:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.catalog = setup_catalog(protocol="file", path=tmp_path / "catalog")
        self.venue_config = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
            # fill_model=fill_model,  # TODO: Implement
        )
        self.data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs_protocol,
            data_cls=QuoteTick,
            instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )
        self.strategies = [
            ImportableStrategyConfig(
                strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
                config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                config={
                    "instrument_id": "AUD/USD.SIM",
                    "bar_type": "AUD/USD.SIM-100-TICK-MID-INTERNAL",
                    "fast_ema_period": 10,
                    "slow_ema_period": 20,
                    "trade_size": "1_000_000",
                    "order_id_tag": "001",
                },
            ),
        ]
        self.backtest_configs = [
            BacktestRunConfig(
                engine=BacktestEngineConfig(
                    strategies=self.strategies,
                    logging=LoggingConfig(bypass_logging=True),
                ),
                venues=[self.venue_config],
                data=[self.data_config],
                chunk_size=5_000,
            ),
        ]
        load_catalog_with_stub_quote_ticks_audusd(self.catalog)  # Load sample data

    def test_init(self):
        # Arrange, Act
        node = BacktestNode(configs=self.backtest_configs)

        # Assert
        assert node

    @pytest.mark.parametrize(
        ("book_type"),
        [
            "L2_MBP",
            "L3_MBO",
        ],
    )
    def test_order_book_with_depth_data_config_validation(self, book_type: str) -> None:
        # Arrange
        venue_l3 = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            book_type=book_type,
            starting_balances=["1_000_000 USD"],
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config, venue_l3],
            data=[self.data_config],
            chunk_size=None,  # No streaming
        )

        with pytest.raises(InvalidConfiguration) as exc_info:
            BacktestNode(configs=[run_config])

        assert (
            str(exc_info.value)
            == f"No order book data available for SIM with book type {book_type}"
        )

    def test_run(self):
        # Arrange
        node = BacktestNode(configs=self.backtest_configs)

        # Act
        results = node.run()

        # Assert
        assert len(results) == 1

    def test_backtest_run_batch_sync(self):
        # Arrange
        config = BacktestRunConfig(
            engine=BacktestEngineConfig(strategies=self.strategies),
            venues=[self.venue_config],
            data=[self.data_config],
            chunk_size=5_000,
        )

        node = BacktestNode(configs=[config])

        # Act
        results = node.run()

        # Assert
        assert len(results) == 1

    def test_backtest_run_results(self):
        # Arrange
        node = BacktestNode(configs=self.backtest_configs)

        # Act
        results = node.run()

        # Assert
        assert isinstance(results, list)
        assert len(results) == 1

    def test_node_config_from_raw(self):
        # Arrange
        raw = msgspec.json.encode(
            {
                "engine": {
                    "trader_id": "Test-111",
                    "log_level": "INFO",
                },
                "venues": [
                    {
                        "name": "SIM",
                        "oms_type": "HEDGING",
                        "account_type": "MARGIN",
                        "base_currency": "USD",
                        "starting_balances": ["1000000 USD"],
                    },
                ],
                "data": [
                    {
                        "catalog_path": "catalog",
                        "data_cls": "nautilus_trader.model.data:QuoteTick",
                        "instrument_id": "AUD/USD.SIM",
                        "start_time": 1580398089820000000,
                        "end_time": 1580504394501000000,
                    },
                ],
                "strategies": [
                    {
                        "strategy_path": "nautilus_trader.examples.strategies.ema_cross:EMACross",
                        "config_path": "nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                        "config": {
                            "instrument_id": "AUD/USD.SIM",
                            "bar_type": "AUD/USD.SIM-100-TICK-MID-INTERNAL",
                            "fast_ema_period": 10,
                            "slow_ema_period": 20,
                            "trade_size": 1_000_000,
                            "order_id_tag": "001",
                        },
                    },
                ],
            },
        )

        # Act
        config = BacktestRunConfig.parse(raw)
        node = BacktestNode(configs=[config])

        # Assert
        node.run()

    def test_backtest_result_total_positions_matches_tearsheet_hedging(self):
        # Arrange
        node = BacktestNode(configs=self.backtest_configs)

        # Act
        results = node.run()
        result = results[0]
        engine = node.get_engines()[0]

        positions = list(engine.kernel.cache.positions())
        snapshots = list(engine.kernel.cache.position_snapshots())
        tearsheet_total = len(positions) + len(snapshots)

        # Assert
        assert result.total_positions == tearsheet_total
        assert result.total_positions == len(positions) + len(snapshots)

    def test_backtest_result_total_positions_matches_tearsheet_netting(self):
        # Arrange
        venue_config_netting = BacktestVenueConfig(
            name="SIM",
            oms_type="NETTING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
        )
        config_netting = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[venue_config_netting],
            data=[self.data_config],
            chunk_size=5_000,
        )
        node = BacktestNode(configs=[config_netting])

        # Act
        results = node.run()
        result = results[0]
        engine = node.get_engines()[0]

        positions = list(engine.kernel.cache.positions())
        snapshots = list(engine.kernel.cache.position_snapshots())
        tearsheet_total = len(positions) + len(snapshots)

        # Assert
        assert result.total_positions == tearsheet_total
        assert result.total_positions == len(positions) + len(snapshots)


class TestBacktestNodeStreaming:
    """
    Tests for BacktestNode streaming mode memory efficiency.
    """

    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.catalog = setup_catalog(protocol="file", path=tmp_path / "catalog")
        self.venue_config = BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
        )
        self.strategies = [
            ImportableStrategyConfig(
                strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
                config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                config={
                    "instrument_id": "AUD/USD.SIM",
                    "bar_type": "AUD/USD.SIM-100-TICK-MID-INTERNAL",
                    "fast_ema_period": 10,
                    "slow_ema_period": 20,
                    "trade_size": "1_000_000",
                    "order_id_tag": "001",
                },
            ),
        ]

    def test_streaming_processes_data_in_chunks(self):
        """
        Verify streaming mode processes multiple chunks, not all data at once.
        """
        # Arrange - load 10K ticks with chunk_size=1000, so should have ~10 chunks
        start_ns, end_ns = load_catalog_with_quote_ticks(self.catalog, count=10_000)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs_protocol,
            data_cls=QuoteTick,
            instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
            start_time=start_ns,
            end_time=end_ns,
        )
        config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config],
            data=[data_config],
            chunk_size=1_000,
        )

        chunk_count = 0
        original_run_streaming = BacktestNode._run_streaming

        def counting_run_streaming(self_node, *args, **kwargs):
            nonlocal chunk_count

            # Instrument the streaming loop by wrapping session.to_query_result
            from nautilus_trader.core.nautilus_pyo3 import DataBackendSession

            original_to_query_result = DataBackendSession.to_query_result

            def counting_to_query_result(session):
                nonlocal chunk_count
                for chunk in original_to_query_result(session):
                    chunk_count += 1
                    yield chunk

            DataBackendSession.to_query_result = counting_to_query_result
            try:
                return original_run_streaming(self_node, *args, **kwargs)
            finally:
                DataBackendSession.to_query_result = original_to_query_result

        # Act
        with patch.object(BacktestNode, "_run_streaming", counting_run_streaming):
            node = BacktestNode(configs=[config])
            node.run()

        # Assert - with 10K ticks and 1K chunk size, should have multiple chunks
        assert chunk_count > 1, f"Expected multiple chunks, got {chunk_count}"

    def test_streaming_clears_data_between_chunks(self):
        """
        Verify clear_data is called during streaming by checking _run_streaming is used.
        """
        # Arrange - load enough data to require multiple chunks
        start_ns, end_ns = load_catalog_with_quote_ticks(self.catalog, count=5_000)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs_protocol,
            data_cls=QuoteTick,
            instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
            start_time=start_ns,
            end_time=end_ns,
        )
        config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config],
            data=[data_config],
            chunk_size=1_000,
        )

        # Track that _run_streaming is called (which contains the clear_data calls)
        streaming_called = False
        original_run_streaming = BacktestNode._run_streaming

        def tracking_run_streaming(self_node, *args, **kwargs):
            nonlocal streaming_called
            streaming_called = True
            return original_run_streaming(self_node, *args, **kwargs)

        # Act
        with patch.object(BacktestNode, "_run_streaming", tracking_run_streaming):
            node = BacktestNode(configs=[config])
            node.run()

        # Assert - _run_streaming should be called when chunk_size is set
        assert streaming_called, "_run_streaming should be called when chunk_size is provided"

    @pytest.mark.slow
    def test_streaming_uses_less_memory_than_oneshot(self):
        """
        Verify streaming mode uses significantly less memory than one-shot mode.

        This test uses 500K real market ticks from the large Tardis dataset to create a
        realistic test scenario. Each mode runs in an isolated subprocess to ensure
        clean memory baselines without allocator arena contamination.

        """
        # Arrange
        start_ns, end_ns = load_catalog_with_large_tardis_quotes(self.catalog, limit=500_000)

        # Build configs as raw dicts for JSON serialization to subprocess
        base_config = {
            "engine": {
                "logging": {"bypass_logging": True},
            },
            "venues": [
                {
                    "name": "BINANCE",
                    "oms_type": "NETTING",
                    "account_type": "MARGIN",
                    "base_currency": "USDT",
                    "starting_balances": ["1000000 USDT"],
                },
            ],
            "data": [
                {
                    "catalog_path": self.catalog.path,
                    "catalog_fs_protocol": self.catalog.fs_protocol,
                    "data_cls": "nautilus_trader.model.data:QuoteTick",
                    "instrument_id": str(_BTCUSDT_HUOBI.id),
                    "start_time": start_ns,
                    "end_time": end_ns,
                },
            ],
            "strategies": [
                {
                    "strategy_path": "nautilus_trader.examples.strategies.ema_cross:EMACross",
                    "config_path": "nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
                    "config": {
                        "instrument_id": str(_BTCUSDT_HUOBI.id),
                        "bar_type": f"{_BTCUSDT_HUOBI.id}-1000-TICK-MID-INTERNAL",
                        "fast_ema_period": 10,
                        "slow_ema_period": 20,
                        "trade_size": "0.01",
                        "order_id_tag": "001",
                    },
                },
            ],
        }

        streaming_config = {**base_config, "chunk_size": 50_000}
        oneshot_config = {**base_config, "chunk_size": None}

        ctx = multiprocessing.get_context("spawn")

        # Streaming mode in subprocess
        streaming_queue: multiprocessing.Queue = ctx.Queue()
        streaming_proc = ctx.Process(
            target=_run_backtest_measure_memory,
            args=(msgspec.json.encode(streaming_config), streaming_queue),
        )
        streaming_proc.start()
        streaming_proc.join(timeout=120)
        assert streaming_proc.exitcode == 0, "Streaming subprocess failed"
        streaming_peak = streaming_queue.get()

        # One-shot mode in subprocess
        oneshot_queue: multiprocessing.Queue = ctx.Queue()
        oneshot_proc = ctx.Process(
            target=_run_backtest_measure_memory,
            args=(msgspec.json.encode(oneshot_config), oneshot_queue),
        )
        oneshot_proc.start()
        oneshot_proc.join(timeout=120)
        assert oneshot_proc.exitcode == 0, "One-shot subprocess failed"
        oneshot_peak = oneshot_queue.get()

        streaming_peak_mb = streaming_peak / 1024 / 1024
        oneshot_peak_mb = oneshot_peak / 1024 / 1024

        # Assert - streaming should use less peak memory than one-shot
        assert streaming_peak < oneshot_peak, (
            f"Streaming peak ({streaming_peak_mb:.1f}MB) should be less than "
            f"one-shot peak ({oneshot_peak_mb:.1f}MB). "
            "This indicates DataFusion may not be streaming data properly."
        )

    def test_streaming_produces_same_results_as_oneshot(self):
        """
        Verify streaming and one-shot modes produce equivalent results.
        """
        # Arrange - load 10K ticks
        start_ns, end_ns = load_catalog_with_quote_ticks(self.catalog, count=10_000)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs_protocol,
            data_cls=QuoteTick,
            instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
            start_time=start_ns,
            end_time=end_ns,
        )

        # Run with streaming (chunk_size=1000)
        streaming_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config],
            data=[data_config],
            chunk_size=1_000,
        )
        streaming_node = BacktestNode(configs=[streaming_config])
        streaming_results = streaming_node.run()

        # Run with one-shot (chunk_size=None)
        oneshot_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config],
            data=[data_config],
            chunk_size=None,
        )
        oneshot_node = BacktestNode(configs=[oneshot_config])
        oneshot_results = oneshot_node.run()

        # Assert - results should be equivalent
        streaming_result = streaming_results[0]
        oneshot_result = oneshot_results[0]

        assert streaming_result.total_orders == oneshot_result.total_orders, (
            f"Order count mismatch: streaming={streaming_result.total_orders}, "
            f"oneshot={oneshot_result.total_orders}"
        )
        assert streaming_result.total_positions == oneshot_result.total_positions, (
            f"Position count mismatch: streaming={streaming_result.total_positions}, "
            f"oneshot={oneshot_result.total_positions}"
        )

    def test_run_streaming_caches_per_catalog(self, monkeypatch, tmp_path):
        monkeypatch.setattr(node, "DataBackendSession", DummyStreamingSession)

        catalog_instances: dict[str, DummyStreamingCatalog] = {}
        instrument_id = InstrumentId.from_str("AUD/USD.SIM")

        def fake_load_catalog(_self, config):
            catalog = DummyStreamingCatalog(config.catalog_path, config.catalog_fs_protocol)
            catalog_instances[config.catalog_path] = catalog
            return catalog

        monkeypatch.setattr(BacktestNode, "load_catalog", fake_load_catalog)

        data_config_a = BacktestDataConfig(
            catalog_path=(tmp_path / "catalog_a").as_posix(),
            catalog_fs_protocol="file",
            data_cls=QuoteTick,
            instrument_id=instrument_id,
            start_time=None,
            end_time=None,
        )

        data_config_b = BacktestDataConfig(
            catalog_path=(tmp_path / "catalog_b").as_posix(),
            catalog_fs_protocol="file",
            data_cls=QuoteTick,
            instrument_id=instrument_id,
            start_time=None,
            end_time=None,
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                strategies=self.strategies,
                logging=LoggingConfig(bypass_logging=True),
            ),
            venues=[self.venue_config],
            data=[data_config_a, data_config_b],
            chunk_size=1_000,
        )

        node_instance = BacktestNode(configs=[run_config])

        class DummyEngine:
            def add_data(self, data, validate=True, sort=True):
                return None

            def run(self, start=None, end=None, run_config_id=None, streaming=None):
                return None

            def clear_data(self):
                return None

            def end(self):
                return None

        node_instance._run_streaming(
            run_config_id=run_config.id,
            engine=DummyStreamingEngine(),
            data_configs=run_config.data,
            chunk_size=run_config.chunk_size,
        )

        assert len(catalog_instances) == 2
        assert catalog_instances[data_config_a.catalog_path].calls == 1
        assert catalog_instances[data_config_b.catalog_path].calls == 1

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

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.trading.strategy import Strategy


@pytest.mark.xdist_group(name="databento_catalog")
class TestBacktestLongRequest:
    def test_long_request_with_time_range_generator(self) -> None:
        # Arrange
        messages_received: list = []

        # Act
        results, future_symbols = run_backtest_long_request(messages_received.append)

        # Assert
        assert results is not None
        assert len(results) == 1

        # Verify bars were received through on_historical_data
        bar_messages = [msg for msg in messages_received if msg.startswith("bar_received:")]
        assert (
            len(bar_messages) > 0
        ), f"Expected to receive historical bars, received messages: {messages_received}"
        assert len(bar_messages) == 10

        # Verify all bars are for the correct instrument
        expected_bar_type = f"{future_symbols[0]}.XCME-1-MINUTE-LAST-EXTERNAL"
        for msg in bar_messages:
            assert (
                expected_bar_type in msg
            ), f"Expected bar type {expected_bar_type} in message {msg}"

        # Verify messages were published
        assert any(
            "long request bars done" in msg for msg in messages_received
        ), f"Expected 'long request bars done' in messages: {messages_received}"


def run_backtest_long_request(test_callback=None):
    """
    Run backtest with long request feature using time_range_generator.
    """
    catalog_folder = TEST_DATA_DIR / "databento" / "historical_bars_catalog"
    catalog = load_catalog(str(catalog_folder))

    future_symbols = ["ESU4"]
    start_time = "2024-07-01T23:40"
    end_time = "2024-07-02T00:10"

    # Load data
    databento_data(
        ["ESU4", "NQU4"],
        start_time,
        end_time,
        "ohlcv-1m",
        "futures",
        str(catalog_folder),
    )

    backtest_start = "2024-07-01T23:55"
    backtest_end = "2024-07-02T00:10"
    historical_start_delay = 10
    historical_end_delay = 1

    strategies = [
        ImportableStrategyConfig(
            strategy_path=LongRequestStrategy.fully_qualified_name(),
            config_path=LongRequestStrategyConfig.fully_qualified_name(),
            config={
                "symbol_id": InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
                "historical_start_delay": historical_start_delay,
                "historical_end_delay": historical_end_delay,
            },
        ),
    ]

    logging = LoggingConfig(
        bypass_logging=True,
    )

    catalogs = [
        DataCatalogConfig(
            path=catalog.path,
        ),
    ]

    data_engine = DataEngineConfig(
        time_bars_origin_offset={
            BarAggregation.MINUTE: pd.Timedelta(seconds=0),
        },
        time_bars_build_delay=0,
    )

    engine_config = BacktestEngineConfig(
        strategies=strategies,
        logging=logging,
        catalogs=catalogs,
        data_engine=data_engine,
    )

    data = [
        BacktestDataConfig(
            data_cls=Bar,
            catalog_path=catalog.path,
            instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
            bar_spec="1-MINUTE-LAST",
            start_time=backtest_start,
            end_time=backtest_end,
        ),
    ]

    venues = [
        BacktestVenueConfig(
            name="XCME",
            oms_type="NETTING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1_000_000 USD"],
        ),
    ]

    configs = [
        BacktestRunConfig(
            engine=engine_config,
            data=data,
            venues=venues,
            chunk_size=None,
            raise_exception=True,
            start=backtest_start,
            end=backtest_end,
        ),
    ]

    node = BacktestNode(configs=configs)
    node.build()

    if test_callback:
        node.get_engine(configs[0].id).kernel.msgbus.subscribe("test", test_callback)

    results = node.run()

    return results, future_symbols


class LongRequestStrategyConfig(StrategyConfig, frozen=True):
    symbol_id: InstrumentId
    historical_start_delay: int = 10
    historical_end_delay: int = 1


class LongRequestStrategy(Strategy):
    def __init__(self, config: LongRequestStrategyConfig):
        super().__init__(config=config)
        self.bars_received: list[Bar] = []

    def on_start(self):
        # Define start and end historical request times
        utc_now = self.clock.utc_now()
        start_historical_bars = utc_now - pd.Timedelta(
            minutes=self.config.historical_start_delay,
        )
        end_historical_bars = utc_now - pd.Timedelta(minutes=self.config.historical_end_delay)
        self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

        # Define bar types
        symbol_id = self.config.symbol_id
        self.external_bar_type = BarType.from_str(f"{symbol_id}-1-MINUTE-LAST-EXTERNAL")

        # Requesting instruments
        self.request_instruments(symbol_id.venue)

        # Test long request with time_range_generator
        self.request_bars(
            self.external_bar_type,
            start_historical_bars,
            end_historical_bars,
            params={
                "time_range_generator": "",  # Use default time range generator
                "durations_seconds": [120],  # Request 2-minute chunks
            },
            callback=lambda x: self.user_log("long request bars done", log_color=LogColor.BLUE),
        )

        self.user_log("request_bars done")

    def on_historical_data(self, data):
        if type(data) is Bar:
            self.bars_received.append(data)
            self.user_log(f"historical bar received: {data.bar_type}")
            self.msgbus.publish(topic="test", msg=f"bar_received:{data.bar_type}")

    def user_log(self, msg, log_color=LogColor.GREEN):
        self.log.warning(str(msg), color=log_color)
        self.msgbus.publish(topic="test", msg=str(msg))


@pytest.mark.xdist_group(name="databento_catalog")
class TestBacktestRequestJoin:
    def test_request_join_with_multiple_instruments(self) -> None:
        # Arrange
        messages_received: list = []

        # Act
        results, future_symbols = run_backtest_request_join(messages_received.append)

        # Assert
        assert results is not None
        assert len(results) == 1

        # Verify bars were received through on_historical_data
        bar_messages = [msg for msg in messages_received if msg.startswith("bar_received:")]
        assert (
            len(bar_messages) > 0
        ), f"Expected to receive historical bars, received messages: {messages_received}"

        # Verify bars are from both instruments
        es_bar_type = f"{future_symbols[0]}.XCME-1-MINUTE-LAST-EXTERNAL"
        nq_bar_type = f"{future_symbols[1]}.XCME-1-MINUTE-LAST-EXTERNAL"

        es_bars = [msg for msg in bar_messages if es_bar_type in msg]
        nq_bars = [msg for msg in bar_messages if nq_bar_type in msg]

        assert len(es_bars) > 0, f"Expected to receive ES bars, received messages: {bar_messages}"
        assert len(nq_bars) > 0, f"Expected to receive NQ bars, received messages: {bar_messages}"
        assert len(es_bars) == 10
        assert len(nq_bars) == 10

        # Verify messages were published
        assert any(
            "join bars done" in msg for msg in messages_received
        ), f"Expected 'join bars done' in messages: {messages_received}"
        assert any(
            "join bars ES done" in msg for msg in messages_received
        ), f"Expected 'join bars ES done' in messages: {messages_received}"
        assert any(
            "join bars NQ done" in msg for msg in messages_received
        ), f"Expected 'join bars NQ done' in messages: {messages_received}"


def run_backtest_request_join(test_callback=None):
    """
    Run backtest with request join feature for multiple instruments.
    """
    catalog_folder = TEST_DATA_DIR / "databento" / "historical_bars_catalog"
    catalog = load_catalog(str(catalog_folder))

    future_symbols = ["ESU4", "NQU4"]
    start_time = "2024-07-01T23:40"
    end_time = "2024-07-02T00:10"

    # Load data
    databento_data(
        future_symbols,
        start_time,
        end_time,
        "ohlcv-1m",
        "futures",
        str(catalog_folder),
    )

    backtest_start = "2024-07-01T23:55"
    backtest_end = "2024-07-02T00:10"
    historical_start_delay = 10
    historical_end_delay = 1

    strategies = [
        ImportableStrategyConfig(
            strategy_path=RequestJoinStrategy.fully_qualified_name(),
            config_path=RequestJoinStrategyConfig.fully_qualified_name(),
            config={
                "symbol_id_1": InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
                "symbol_id_2": InstrumentId.from_str(f"{future_symbols[1]}.XCME"),
                "historical_start_delay": historical_start_delay,
                "historical_end_delay": historical_end_delay,
            },
        ),
    ]

    logging = LoggingConfig(
        bypass_logging=True,
    )

    catalogs = [
        DataCatalogConfig(
            path=catalog.path,
        ),
    ]

    data_engine = DataEngineConfig(
        time_bars_origin_offset={
            BarAggregation.MINUTE: pd.Timedelta(seconds=0),
        },
        time_bars_build_delay=0,
    )

    engine_config = BacktestEngineConfig(
        strategies=strategies,
        logging=logging,
        catalogs=catalogs,
        data_engine=data_engine,
    )

    data = [
        BacktestDataConfig(
            data_cls=Bar,
            catalog_path=catalog.path,
            instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
            bar_spec="1-MINUTE-LAST",
            start_time=backtest_start,
            end_time=backtest_end,
        ),
        BacktestDataConfig(
            data_cls=Bar,
            catalog_path=catalog.path,
            instrument_id=InstrumentId.from_str(f"{future_symbols[1]}.XCME"),
            bar_spec="1-MINUTE-LAST",
            start_time=backtest_start,
            end_time=backtest_end,
        ),
    ]

    venues = [
        BacktestVenueConfig(
            name="XCME",
            oms_type="NETTING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1_000_000 USD"],
        ),
    ]

    configs = [
        BacktestRunConfig(
            engine=engine_config,
            data=data,
            venues=venues,
            chunk_size=None,
            raise_exception=True,
            start=backtest_start,
            end=backtest_end,
        ),
    ]

    node = BacktestNode(configs=configs)
    node.build()

    if test_callback:
        node.get_engine(configs[0].id).kernel.msgbus.subscribe("test", test_callback)

    results = node.run()

    return results, future_symbols


class RequestJoinStrategyConfig(StrategyConfig, frozen=True):
    symbol_id_1: InstrumentId
    symbol_id_2: InstrumentId
    historical_start_delay: int = 10
    historical_end_delay: int = 1


class RequestJoinStrategy(Strategy):
    def __init__(self, config: RequestJoinStrategyConfig):
        super().__init__(config=config)
        self.bars_received: list[Bar] = []

    def on_start(self):
        # Define start and end historical request times
        utc_now = self.clock.utc_now()
        start_historical_bars = utc_now - pd.Timedelta(
            minutes=self.config.historical_start_delay,
        )
        end_historical_bars = utc_now - pd.Timedelta(minutes=self.config.historical_end_delay)
        self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

        # Define bar types
        self.external_bar_type_1 = BarType.from_str(
            f"{self.config.symbol_id_1}-1-MINUTE-LAST-EXTERNAL",
        )
        self.external_bar_type_2 = BarType.from_str(
            f"{self.config.symbol_id_2}-1-MINUTE-LAST-EXTERNAL",
        )

        # Requesting instruments
        self.request_instruments(self.config.symbol_id_1.venue)

        # Test request_join
        uuid_1 = self.request_bars(
            self.external_bar_type_1,
            unix_nanos_to_dt(0),
            join_request=True,
            callback=lambda x: self.user_log("join bars ES done", log_color=LogColor.BLUE),
        )
        uuid_2 = self.request_bars(
            self.external_bar_type_2,
            unix_nanos_to_dt(0),
            join_request=True,
            callback=lambda x: self.user_log("join bars NQ done", log_color=LogColor.BLUE),
        )
        self.request_join(
            (uuid_1, uuid_2),
            start_historical_bars,
            end_historical_bars,
            params={
                "time_range_generator": "",  # Use default time range generator
                "durations_seconds": [120],  # Request 2-minute chunks
            },
            callback=lambda x: self.user_log("join bars done", log_color=LogColor.BLUE),
        )

        self.user_log("request_join done")

    def on_historical_data(self, data):
        if type(data) is Bar:
            self.bars_received.append(data)
            self.user_log(f"historical bar received: {data.bar_type}")
            self.msgbus.publish(topic="test", msg=f"bar_received:{data.bar_type}")

    def user_log(self, msg, log_color=LogColor.GREEN):
        self.log.warning(str(msg), color=log_color)
        self.msgbus.publish(topic="test", msg=str(msg))

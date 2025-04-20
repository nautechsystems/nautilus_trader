import pandas as pd

from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.engine import BacktestDataIterator
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.greeks_data import GreeksData
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.types import CatalogWriteMode
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.test_kit.stubs.data import MyData
from nautilus_trader.trading.strategy import Strategy


class TestBacktestDataIterator:
    def test_backtest_data_iterator(self):
        # Arrange

        data_iterator = BacktestDataIterator()

        data_len = 5
        data_0 = [MyData(0, ts_init=3 * k) for k in range(data_len)]
        data_1 = [MyData(0, ts_init=3 * k + 1) for k in range(data_len)]
        data_2 = [MyData(0, ts_init=3 * k + 2) for k in range(data_len)]

        # Act - Add data
        data_iterator.add_data("base", data_0)
        data_iterator.add_data("extra_1", data_1)
        data_iterator.add_data("extra_2", data_2)

        # Assert - Iterate through data
        data_result = list(data_iterator)
        assert len(data_result) == 15  # 5 items from each of the 3 data sources

        # Verify the data is sorted by ts_init
        for i in range(len(data_result) - 1):
            assert data_result[i].ts_init <= data_result[i + 1].ts_init

        # Act - Reset and iterate again
        data_iterator.reset()
        data_result_2 = list(data_iterator)

        # Assert - Same results after reset
        assert len(data_result_2) == 15
        assert [x.ts_init for x in data_result] == [x.ts_init for x in data_result_2]

        # Act - Test all_data method
        all_data = data_iterator.all_data()

        # Assert - Check all_data returns correct data
        assert len(all_data) == 3
        assert "base" in all_data
        assert "extra_1" in all_data
        assert "extra_2" in all_data
        assert all_data["base"] == data_0
        assert all_data["extra_1"] == data_1
        assert all_data["extra_2"] == data_2

        # Act - Test remove_data
        data_iterator.remove_data("extra_1")
        data_iterator.reset()
        data_result_3 = list(data_iterator)

        # Assert - Correct data after removal
        assert len(data_result_3) == 10  # 5 items from each of the 2 remaining data sources

        # Act - Remove all data
        data_iterator.remove_data("base")
        data_iterator.remove_data("extra_2")
        data_iterator.reset()
        data_result_4 = list(data_iterator)

        # Assert - No data left
        assert len(data_result_4) == 0

    def test_backtest_data_iterator_callback(self):
        # Arrange

        callback_data = []

        def empty_data_callback(data_name, last_ts_init):
            callback_data.append((data_name, last_ts_init))

        data_iterator = BacktestDataIterator(empty_data_callback=empty_data_callback)

        # Create data with different lengths
        data_0 = [MyData(0, ts_init=k) for k in range(3)]  # 0, 1, 2
        data_1 = [MyData(0, ts_init=k) for k in range(5)]  # 0, 1, 2, 3, 4

        # Act - Add data
        data_iterator.add_data("short", data_0)
        data_iterator.add_data("long", data_1)

        # Consume all data
        _ = list(data_iterator)

        # Assert - Callbacks were called for both data streams
        # The callback is called when we try to access data beyond what's available
        assert len(callback_data) == 2

        # Check that both data streams triggered callbacks
        data_names = [item[0] for item in callback_data]
        assert "short" in data_names
        assert "long" in data_names


class TestBacktestNodeWithBacktestDataIterator:
    def test_backtest_same_with_and_without_data_configs(self) -> None:
        # Arrange
        messages_with_data: list = []
        messages_without_data: list = []

        # Act
        run_backtest(messages_with_data.append, with_data=True)
        run_backtest(messages_without_data.append, with_data=False)

        assert messages_with_data == messages_without_data


def run_backtest(test_callback=None, with_data=True, log_path=None):
    catalog_folder = "options_catalog"
    catalog = load_catalog(catalog_folder)

    future_symbols = ["ESM4"]
    option_symbols = ["ESM4 P5230", "ESM4 P5250"]

    start_time = "2024-05-09T10:00"
    end_time = "2024-05-09T10:05"

    _ = databento_data(
        future_symbols,
        start_time,
        end_time,
        "ohlcv-1m",
        "futures",
        catalog_folder,
    )
    _ = databento_data(
        option_symbols,
        start_time,
        end_time,
        "bbo-1m",
        "options",
        catalog_folder,
    )

    # for saving and loading custom data greeks, use True, False then False, True below
    stream_data, load_greeks = False, False

    # actors = [
    #     ImportableActorConfig(
    #         actor_path=InterestRateProvider.fully_qualified_name(),
    #         config_path=InterestRateProviderConfig.fully_qualified_name(),
    #         config={
    #             "interest_rates_file": str(
    #                 data_path(catalog_folder, "usd_short_term_rate.xml"),
    #             ),
    #         },
    #     ),
    # ]

    strategies = [
        ImportableStrategyConfig(
            strategy_path=OptionStrategy.fully_qualified_name(),
            config_path=OptionConfig.fully_qualified_name(),
            config={
                "future_id": InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
                "option_id": InstrumentId.from_str(f"{option_symbols[0]}.GLBX"),
                "option_id2": InstrumentId.from_str(f"{option_symbols[1]}.GLBX"),
                "load_greeks": load_greeks,
            },
        ),
    ]

    streaming = StreamingConfig(
        catalog_path=catalog.path,
        fs_protocol="file",
        include_types=[GreeksData],
    )

    logging = LoggingConfig(
        bypass_logging=False,
        log_colors=True,
        log_level="WARN",
        log_level_file="WARN",
        log_directory=log_path,  # must be the same as conftest.py
        log_file_format=None,  # "json" or None
        log_file_name="test_logs",  # must be the same as conftest.py
        clear_log_file=True,
        print_config=False,
        use_pyo3=False,
    )

    catalogs = [
        DataCatalogConfig(
            path=catalog.path,
        ),
    ]

    engine_config = BacktestEngineConfig(
        logging=logging,
        # actors=actors,
        strategies=strategies,
        streaming=(streaming if stream_data else None),
        catalogs=catalogs,
    )

    if with_data:
        data = [
            BacktestDataConfig(
                data_cls=QuoteTick,
                catalog_path=catalog.path,
                instrument_id=InstrumentId.from_str(f"{option_symbols[0]}.GLBX"),
            ),
            BacktestDataConfig(
                data_cls=QuoteTick,
                catalog_path=catalog.path,
                instrument_id=InstrumentId.from_str(f"{option_symbols[1]}.GLBX"),
            ),
            BacktestDataConfig(
                data_cls=Bar,
                catalog_path=catalog.path,
                instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
                bar_spec="1-MINUTE-LAST",
            ),
        ]
    else:
        data = []

    if load_greeks:
        data = [
            BacktestDataConfig(
                data_cls=GreeksData.fully_qualified_name(),
                catalog_path=catalog.path,
                client_id="GreeksDataProvider",
                metadata={"instrument_id": "ES"},
            ),
            *data,
        ]

    venues = [
        BacktestVenueConfig(
            name="GLBX",
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
            chunk_size=None,  # use None when loading custom data, else a value of 10_000 for example
            start=start_time,
            end=end_time,
        ),
    ]

    node = BacktestNode(configs=configs)
    node.build()

    if test_callback:
        node.get_engine(configs[0].id).kernel.msgbus.subscribe("test", test_callback)

    results = node.run()

    if stream_data:
        catalog.convert_stream_to_data(
            results[0].instance_id,
            GreeksData,
            mode=CatalogWriteMode.NEWFILE,
        )

    engine: BacktestEngine = node.get_engine(configs[0].id)
    engine.trader.generate_order_fills_report()
    engine.trader.generate_positions_report()
    engine.trader.generate_account_report(Venue("GLBX"))
    node.dispose()


class OptionConfig(StrategyConfig, frozen=True):
    future_id: InstrumentId
    option_id: InstrumentId
    option_id2: InstrumentId
    load_greeks: bool = False


class OptionStrategy(Strategy):
    def __init__(self, config: OptionConfig):
        super().__init__(config=config)
        self.start_orders_done = False

    def on_start(self):
        self.bar_type = BarType.from_str(f"{self.config.future_id}-1-MINUTE-LAST-EXTERNAL")

        self.request_instrument(self.config.option_id)
        self.request_instrument(self.config.option_id2)
        self.request_instrument(self.bar_type.instrument_id)

        self.subscribe_quote_ticks(self.config.option_id2)
        self.subscribe_quote_ticks(
            self.config.option_id,
            params={
                "duration_seconds": pd.Timedelta(minutes=1).seconds,
                "append_data": False,
            },
        )
        self.subscribe_bars(self.bar_type)

        if self.config.load_greeks:
            self.greeks.subscribe_greeks("ES")

    def on_quote_tick(self, data):
        self.user_log(data)

    def init_portfolio(self):
        self.submit_market_order(instrument_id=self.config.option_id, quantity=-10)
        self.submit_market_order(instrument_id=self.config.option_id2, quantity=10)
        self.submit_market_order(instrument_id=self.config.future_id, quantity=1)

        self.start_orders_done = True

    # def on_bar(self, data):
    #     self.user_log(data)

    def on_bar(self, bar):
        self.user_log(
            f"bar ts_init = {unix_nanos_to_iso8601(bar.ts_init)}, bar close = {bar.close}",
        )

        if not self.start_orders_done:
            self.user_log("Initializing the portfolio with some trades")
            self.init_portfolio()
            return

        self.display_greeks()

    def display_greeks(self, alert=None):
        portfolio_greeks = self.greeks.portfolio_greeks(
            use_cached_greeks=self.config.load_greeks,
            publish_greeks=(not self.config.load_greeks),
        )
        self.user_log(f"{portfolio_greeks=}")

    def submit_market_order(self, instrument_id, quantity):
        order = self.order_factory.market(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
        )

        self.submit_order(order)

    def submit_limit_order(self, instrument_id, price, quantity):
        order = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
            price=Price(price),
        )

        self.submit_order(order)

    def user_log(self, msg):
        self.log.warning(str(msg), color=LogColor.GREEN)
        self.msgbus.publish(topic="test", msg=str(msg))

    def on_stop(self):
        self.unsubscribe_bars(self.bar_type)

import datetime
from unittest.mock import patch

import pytest
from ib_insync import Contract
from ib_insync import TickAttribBidAsk
from ib_insync import TickByTickBidAsk

from nautilus_trader.model.enums import BookType
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()

    def instrument_setup(self, instrument, contract_details):
        self.data_client.instrument_provider.contract_details[instrument.id] = contract_details
        self.data_client.instrument_provider.contract_id_to_instrument_id[
            contract_details.contract.conId
        ] = instrument.id

    def _async_setup(self, loop):
        pass

    #     # Fixture Setup
    #     self.loop = loop
    #     self.clock = LiveClock()
    #     self.logger = LiveLogger(
    #         loop=self.loop,
    #         clock=self.clock,
    #         level_stdout=LogLevel.DEBUG,
    #     )
    #
    #     self.trader_id = TestStubs.trader_id()
    #     self.strategy_id = TestStubs.strategy_id()
    #     self.account_id = TestStubs.account_id()
    #
    #     self.msgbus = MessageBus(
    #         trader_id=self.trader_id,
    #         clock=self.clock,
    #         logger=self.logger,
    #     )
    #
    #     self.cache_db = MockCacheDatabase(
    #         logger=self.logger,
    #     )
    #
    #     self.cache = Cache(
    #         database=self.cache_db,
    #         logger=self.logger,
    #     )
    #
    #     # Arrange, Act
    #     with patch("nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client"):
    #
    #         self.data_client = InteractiveBrokersLiveDataClientFactory.create(
    #             loop=self.loop,
    #             name="IB",
    #             config={},
    #             msgbus=self.msgbus,
    #             cache=self.cache,
    #             clock=self.clock,
    #             logger=self.logger,
    #         )

    @pytest.mark.asyncio
    async def test_factory(self, event_loop):
        # Arrange
        self._async_setup(loop=event_loop)

        # Act
        data_client = self.data_client

        # Assert
        assert data_client is not None

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(self, event_loop):
        # Arrange
        instrument_aapl = IBTestStubs.instrument(symbol="AAPL")
        self._async_setup(loop=event_loop)
        self.data_client.instrument_provider.contract_details[
            instrument_aapl.id
        ] = IBTestStubs.contract_details("AAPL")

        # Act
        with patch.object(self.data_client, "_client") as mock:
            self.data_client.subscribe_trade_ticks(instrument_id=instrument_aapl.id)

        # Assert
        mock_call = mock.method_calls[0]
        assert mock_call[0] == "reqMktData"
        assert mock_call[1] == ()
        assert mock_call[2] == {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
        }

    @pytest.mark.asyncio
    async def test_subscribe_order_book_deltas(self, event_loop):
        # Arrange
        instrument = IBTestStubs.instrument(symbol="AAPL")
        self.instrument_setup(instrument, IBTestStubs.contract_details("AAPL"))

        # Act
        with patch.object(self.data_client, "_client") as mock:
            self.data_client.subscribe_order_book_snapshots(
                instrument_id=instrument.id, book_type=BookType.L2_MBP
            )

        # Assert
        mock_call = mock.method_calls[0]
        assert mock_call[0] == "reqMktDepth"
        assert mock_call[1] == ()
        assert mock_call[2] == {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "numRows": 5,
        }

    @pytest.mark.asyncio
    async def test_on_book_update(self, event_loop):
        # Arrange
        self.instrument_setup(
            IBTestStubs.instrument(symbol="EURUSD"), IBTestStubs.contract_details("EURUSD")
        )

        # Act
        for ticker in IBTestStubs.market_depth(name="eurusd"):
            self.data_client._on_order_book_snapshot(ticker=ticker, book_type=BookType.L2_MBP)

    @pytest.mark.asyncio
    async def test_on_ticker_update(self, event_loop):
        # Arrange
        self.instrument_setup(
            IBTestStubs.instrument(symbol="EURUSD"), IBTestStubs.contract_details("EURUSD")
        )

        # Act
        for ticker in IBTestStubs.tickers("eurusd"):
            self.data_client._on_trade_ticker_update(ticker=ticker)

    @pytest.mark.asyncio
    async def test_on_quote_tick_update(self, event_loop):
        # Arrange
        self.instrument_setup(
            IBTestStubs.instrument(symbol="EURUSD"), IBTestStubs.contract_details("EURUSD")
        )
        contract = IBTestStubs.contract_details("EURUSD").contract
        quote_tick = TickByTickBidAsk(
            time=datetime.datetime(2022, 3, 4, 6, 8, 36, 992576, tzinfo=datetime.timezone.utc),
            bidPrice=99.45,
            askPrice=99.5,
            bidSize=44600.0,
            askSize=29500.0,
            tickAttribBidAsk=TickAttribBidAsk(bidPastLow=False, askPastHigh=False),
        )

        # Act
        self.data_client._on_quote_tick_update(tick=quote_tick, contract=contract)

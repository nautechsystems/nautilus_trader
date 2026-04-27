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

import pandas as pd
import pytest
import pytz

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.engine import DataEngineConfig
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import SubscribeOptionChain
from nautilus_trader.data.messages import SubscribeOptionGreeks
from nautilus_trader.data.messages import UnsubscribeOptionChain
from nautilus_trader.data.messages import UnsubscribeOptionGreeks
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OptionGreeks
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.option_contract import OptionContract
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


OPRA = Venue("OPRA")
EXPIRY_NS = pd.Timestamp("2024-03-15", tz=pytz.utc).value


class MockOptionDataClient(MarketDataClient):
    """
    MarketDataClient that tracks option greeks subscriptions without raising.
    """

    def __init__(self, client_id, msgbus, cache, clock):
        super().__init__(
            client_id=client_id,
            venue=Venue(str(client_id)),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        self._set_connected()

    def subscribe_quote_ticks(self, command):
        self._add_subscription_quote_ticks(command.instrument_id)

    def unsubscribe_quote_ticks(self, command):
        self._remove_subscription_quote_ticks(command.instrument_id)

    def subscribe_option_greeks(self, command):
        self._add_subscription_option_greeks(command.instrument_id)

    def unsubscribe_option_greeks(self, command):
        self._remove_subscription_option_greeks(command.instrument_id)

    def subscribe_instrument_status(self, command):
        self._add_subscription_instrument_status(command.instrument_id)

    def unsubscribe_instrument_status(self, command):
        self._remove_subscription_instrument_status(command.instrument_id)

    def request_forward_prices(self, request):
        pass  # no-op for tests


def _make_option(
    symbol: str,
    underlying: str,
    strike: str,
    kind: OptionKind,
    expiry_ns: int = EXPIRY_NS,
    venue: Venue = OPRA,
) -> OptionContract:
    return OptionContract(
        instrument_id=InstrumentId(Symbol(symbol), venue),
        raw_symbol=Symbol(symbol),
        asset_class=AssetClass.EQUITY,
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        underlying=underlying,
        option_kind=kind,
        strike_price=Price.from_str(strike),
        activation_ns=0,
        expiration_ns=expiry_ns,
        ts_event=0,
        ts_init=0,
    )


def _make_greeks(instrument_id: InstrumentId) -> OptionGreeks:
    return OptionGreeks(
        instrument_id=instrument_id,
        delta=0.55,
        gamma=0.02,
        vega=0.15,
        theta=-0.05,
        rho=0.01,
        mark_iv=0.25,
        bid_iv=0.24,
        ask_iv=0.26,
        underlying_price=155.0,
        open_interest=1000.0,
        ts_event=0,
        ts_init=0,
    )


class TestOptionChainEngine:
    @pytest.fixture(autouse=True)
    def setup(self):
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=DataEngineConfig(debug=True),
        )
        self.client = MockOptionDataClient(
            client_id=ClientId("OPRA"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine.register_client(self.client)
        self.client.start()

        self.aapl_call_150 = _make_option("AAPL240315C150", "AAPL", "150.00", OptionKind.CALL)
        self.aapl_put_150 = _make_option("AAPL240315P150", "AAPL", "150.00", OptionKind.PUT)
        self.aapl_call_155 = _make_option("AAPL240315C155", "AAPL", "155.00", OptionKind.CALL)

        # Same expiry/settlement as AAPL but different underlying
        self.msft_call_400 = _make_option("MSFT240315C400", "MSFT", "400.00", OptionKind.CALL)
        self.msft_put_400 = _make_option("MSFT240315P400", "MSFT", "400.00", OptionKind.PUT)

        for inst in [
            self.aapl_call_150,
            self.aapl_put_150,
            self.aapl_call_155,
            self.msft_call_400,
            self.msft_put_400,
        ]:
            self.data_engine.process(inst)

        self.series_id = nautilus_pyo3.OptionSeriesId(
            "OPRA",
            "AAPL",
            "USD",
            EXPIRY_NS,
        )

    _UNSET = object()

    def _subscribe_and_bootstrap(self, series_id, strike_range=_UNSET, snapshot_interval_ms=None):
        """
        Subscribe to an option chain and complete the bootstrap by simulating an empty
        forward price response.
        """
        if strike_range is self._UNSET:
            strike_range = nautilus_pyo3.StrikeRange.atm_relative(100, 100)

        sub_cmd = SubscribeOptionChain(
            series_id=series_id,
            strike_range=strike_range,
            snapshot_interval_ms=snapshot_interval_ms,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(sub_cmd)

        pending = dict(self.data_engine._pending_option_chain_requests)
        for corr_id in pending:
            response = DataResponse(
                client_id=self.client.id,
                venue=OPRA,
                data_type=DataType(Data),
                data=[],
                correlation_id=corr_id,
                response_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                start=None,
                end=None,
            )
            self.data_engine._handle_response(response)

    def test_subscribe_option_chain_none_strike_range(self):
        # Arrange, Act
        command = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=None,
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert command.series_id == self.series_id
        assert command.strike_range is None

    def test_subscribe_option_chain_with_none_strike_range_executes(self):
        # Arrange
        command = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=None,
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(command)

        # Assert
        assert len(self.data_engine._pending_option_chain_requests) == 1

    def test_subscribe_option_chain_none_strike_range_completes_bootstrap(self):
        # Act
        self._subscribe_and_bootstrap(self.series_id, strike_range=None)

        # Assert
        series_key = str(self.series_id)
        assert series_key in self.data_engine._option_chain_managers

        manager = self.data_engine._option_chain_managers[series_key]
        all_ids = [str(iid) for iid in manager.all_instrument_ids()]
        assert len(all_ids) == 3  # 150C, 150P, 155C

    def test_subscribe_option_chain_fixed_strike_range_skips_bootstrap(self):
        # Arrange
        strike_range = nautilus_pyo3.StrikeRange.fixed(
            [
                nautilus_pyo3.Price.from_str("150.00"),
                nautilus_pyo3.Price.from_str("155.00"),
            ],
        )
        command = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=strike_range,
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(command)

        # Assert: Fixed does not need ATM bootstrap, so no pending forward-price
        # request should be created and the manager exists immediately.
        assert len(self.data_engine._pending_option_chain_requests) == 0
        assert str(self.series_id) in self.data_engine._option_chain_managers

    def test_subscribe_option_chain_atm_relative_creates_pending_request(self):
        # Arrange
        strike_range = nautilus_pyo3.StrikeRange.atm_relative(2, 2)
        command = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=strike_range,
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(command)

        # Assert: AtmRelative needs ATM bootstrap, so a forward-price request is pending.
        assert len(self.data_engine._pending_option_chain_requests) == 1
        assert str(self.series_id) not in self.data_engine._option_chain_managers

    def test_option_chain_resolves_only_matching_underlying(self):
        # Act
        self._subscribe_and_bootstrap(self.series_id)

        # Assert
        series_key = str(self.series_id)
        manager = self.data_engine._option_chain_managers[series_key]
        all_ids = [str(iid) for iid in manager.all_instrument_ids()]

        assert any("AAPL" in iid for iid in all_ids)
        assert not any("MSFT" in iid for iid in all_ids)

    def test_option_chain_does_not_mix_underlyings_same_expiry(self):
        # Arrange
        msft_series = nautilus_pyo3.OptionSeriesId(
            "OPRA",
            "MSFT",
            "USD",
            EXPIRY_NS,
        )

        # Act
        self._subscribe_and_bootstrap(self.series_id)
        self._subscribe_and_bootstrap(msft_series)

        # Assert
        aapl_manager = self.data_engine._option_chain_managers[str(self.series_id)]
        msft_manager = self.data_engine._option_chain_managers[str(msft_series)]

        aapl_ids = {str(iid) for iid in aapl_manager.all_instrument_ids()}
        msft_ids = {str(iid) for iid in msft_manager.all_instrument_ids()}

        assert aapl_ids.isdisjoint(msft_ids)
        assert len(aapl_ids) == 3  # 150C, 150P, 155C
        assert len(msft_ids) == 2  # 400C, 400P

    def test_unsubscribe_option_chain_with_remaining_subscriber_preserves_manager(self):
        # Arrange
        series_key = str(self.series_id)
        self._subscribe_and_bootstrap(self.series_id)
        assert series_key in self.data_engine._option_chain_managers

        topic = f"data.option_chain.{series_key}"
        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic=topic, handler=handler1.append)
        self.msgbus.subscribe(topic=topic, handler=handler2.append)
        self.msgbus.unsubscribe(topic=topic, handler=handler1.append)

        unsub_cmd = UnsubscribeOptionChain(
            series_id=self.series_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsub_cmd)

        # Assert
        assert series_key in self.data_engine._option_chain_managers

    def test_unsubscribe_option_chain_last_subscriber_tears_down(self):
        # Arrange
        series_key = str(self.series_id)
        self._subscribe_and_bootstrap(self.series_id)
        assert series_key in self.data_engine._option_chain_managers

        topic = f"data.option_chain.{series_key}"
        handler = []
        self.msgbus.subscribe(topic=topic, handler=handler.append)
        self.msgbus.unsubscribe(topic=topic, handler=handler.append)

        unsub_cmd = UnsubscribeOptionChain(
            series_id=self.series_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsub_cmd)

        # Assert
        assert series_key not in self.data_engine._option_chain_managers

    def test_unsubscribe_option_chain_preserves_pending_when_subscribers_remain(self):
        # Arrange
        sub_cmd = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=None,
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(sub_cmd)
        assert len(self.data_engine._pending_option_chain_requests) == 1

        series_key = str(self.series_id)
        topic = f"data.option_chain.{series_key}"
        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic=topic, handler=handler1.append)
        self.msgbus.subscribe(topic=topic, handler=handler2.append)
        self.msgbus.unsubscribe(topic=topic, handler=handler1.append)

        unsub_cmd = UnsubscribeOptionChain(
            series_id=self.series_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsub_cmd)

        # Assert
        assert len(self.data_engine._pending_option_chain_requests) == 1

    def test_unsubscribe_option_chain_clears_pending_bootstrap(self):
        # Arrange
        sub_cmd = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=None,
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(sub_cmd)
        assert len(self.data_engine._pending_option_chain_requests) == 1

        unsub_cmd = UnsubscribeOptionChain(
            series_id=self.series_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsub_cmd)

        # Assert
        assert len(self.data_engine._pending_option_chain_requests) == 0

    def test_option_greeks_published_to_bus(self):
        # Arrange
        greeks = _make_greeks(self.aapl_call_150.id)
        received = []
        topic = f"data.option_greeks.{self.aapl_call_150.id.venue}.{self.aapl_call_150.id.symbol}"
        self.msgbus.subscribe(topic=topic, handler=received.append)

        # Act
        self.data_engine.process(greeks)

        # Assert
        assert len(received) == 1
        assert received[0].instrument_id == self.aapl_call_150.id

    def test_unsubscribe_option_greeks_with_remaining_subscriber_preserves_client_sub(self):
        # Arrange
        inst_id = self.aapl_call_150.id
        topic = f"data.option_greeks.{inst_id.venue}.{inst_id.symbol}"

        handler1 = []
        handler2 = []
        self.msgbus.subscribe(topic=topic, handler=handler1.append)
        self.msgbus.subscribe(topic=topic, handler=handler2.append)

        sub_cmd = SubscribeOptionGreeks(
            instrument_id=inst_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(sub_cmd)
        assert inst_id in self.client.subscribed_option_greeks()

        self.msgbus.unsubscribe(topic=topic, handler=handler1.append)

        unsub_cmd = UnsubscribeOptionGreeks(
            instrument_id=inst_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsub_cmd)

        # Assert
        assert inst_id in self.client.subscribed_option_greeks()

    def test_unsubscribe_option_greeks_last_subscriber_removes_client_sub(self):
        # Arrange
        inst_id = self.aapl_call_150.id
        topic = f"data.option_greeks.{inst_id.venue}.{inst_id.symbol}"

        handler = []
        self.msgbus.subscribe(topic=topic, handler=handler.append)

        sub_cmd = SubscribeOptionGreeks(
            instrument_id=inst_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.data_engine.execute(sub_cmd)
        assert inst_id in self.client.subscribed_option_greeks()

        self.msgbus.unsubscribe(topic=topic, handler=handler.append)

        unsub_cmd = UnsubscribeOptionGreeks(
            instrument_id=inst_id,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.execute(unsub_cmd)

        # Assert
        assert inst_id not in self.client.subscribed_option_greeks()

    def test_snapshot_timer_tears_down_expired_series(self):
        # Arrange: set clock close to expiry so advance doesn't generate billions of events
        self.clock.set_time(EXPIRY_NS - 5_000_000_000)  # 5 seconds before expiry

        series_key = str(self.series_id)
        self._subscribe_and_bootstrap(self.series_id, snapshot_interval_ms=1000)

        assert series_key in self.data_engine._option_chain_managers
        assert series_key in self.data_engine._option_chain_timer_names

        # Act: advance clock past expiration and fire the timer
        events = self.clock.advance_time(EXPIRY_NS + 1_000_000_000)
        for event in events:
            event.handle()

        # Assert: manager, timer, and instrument index all cleaned up
        assert series_key not in self.data_engine._option_chain_managers
        assert series_key not in self.data_engine._option_chain_timer_names
        for sk in self.data_engine._option_chain_instrument_index.values():
            assert sk != series_key

    def test_snapshot_timer_publishes_slice_to_bus(self):
        # Arrange: subscribe with snapshot interval
        series_key = str(self.series_id)
        self._subscribe_and_bootstrap(self.series_id, snapshot_interval_ms=1000)

        # Feed greeks with underlying_price to trigger ATM bootstrap
        greeks = _make_greeks(self.aapl_call_150.id)
        self.data_engine.process(greeks)

        # Feed a quote tick so the aggregator has data for the snapshot
        quote = QuoteTick(
            self.aapl_call_150.id,
            Price.from_str("5.00"),
            Price.from_str("5.50"),
            Quantity.from_int(10),
            Quantity.from_int(10),
            0,
            0,
        )
        self.data_engine.process(quote)

        # Subscribe to the option chain topic
        received = []
        topic = f"data.option_chain.{series_key}"
        self.msgbus.subscribe(topic=topic, handler=received.append)

        # Act: advance clock by 1 second to trigger the snapshot timer
        events = self.clock.advance_time(1_000_000_000)
        for event in events:
            event.handle()

        # Assert: snapshot was published to the bus
        assert len(received) == 1


class TestStrikeRangeKind:
    def test_fixed_kind(self):
        strike_range = nautilus_pyo3.StrikeRange.fixed(
            [nautilus_pyo3.Price.from_str("100.00")],
        )
        assert strike_range.kind == "Fixed"

    def test_atm_relative_kind(self):
        strike_range = nautilus_pyo3.StrikeRange.atm_relative(2, 2)
        assert strike_range.kind == "AtmRelative"

    def test_atm_percent_kind(self):
        strike_range = nautilus_pyo3.StrikeRange.atm_percent(0.05)
        assert strike_range.kind == "AtmPercent"


class TestOptionChainBacktestIntegration:
    """
    Regression coverage for issue #3938: BacktestMarketDataClient must unblock the
    engine's pending option-chain bootstrap so the manager is created end-to-end without
    manual response feeding.
    """

    @pytest.fixture(autouse=True)
    def setup(self):
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.client = BacktestMarketDataClient(
            client_id=ClientId("OPRA"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.data_engine.register_client(self.client)
        self.client.start()

        for inst in [
            _make_option("AAPL240315C150", "AAPL", "150.00", OptionKind.CALL),
            _make_option("AAPL240315P150", "AAPL", "150.00", OptionKind.PUT),
            _make_option("AAPL240315C155", "AAPL", "155.00", OptionKind.CALL),
        ]:
            self.data_engine.process(inst)

        self.series_id = nautilus_pyo3.OptionSeriesId(
            "OPRA",
            "AAPL",
            "USD",
            EXPIRY_NS,
        )

    def test_atm_relative_subscription_unblocks_via_backtest_client(self):
        # Arrange
        sub_cmd = SubscribeOptionChain(
            series_id=self.series_id,
            strike_range=nautilus_pyo3.StrikeRange.atm_relative(2, 2),
            snapshot_interval_ms=None,
            client_id=self.client.id,
            venue=OPRA,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act: response routes through msgbus synchronously, so the engine
        # processes it before execute() returns
        self.data_engine.execute(sub_cmd)

        # Assert
        assert len(self.data_engine._pending_option_chain_requests) == 0
        assert str(self.series_id) in self.data_engine._option_chain_managers

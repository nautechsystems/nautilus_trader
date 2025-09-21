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

"""
This module provides a data client for backtesting.
"""

from libc.stdint cimport uint64_t

from nautilus_trader.common.config import NautilusConfig

from nautilus_trader.backtest.models cimport SpreadQuoteAggregator
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport RequestBars
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport RequestInstrument
from nautilus_trader.data.messages cimport RequestInstruments
from nautilus_trader.data.messages cimport RequestOrderBookSnapshot
from nautilus_trader.data.messages cimport RequestQuoteTicks
from nautilus_trader.data.messages cimport RequestTradeTicks
from nautilus_trader.data.messages cimport SubscribeBars
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeFundingRates
from nautilus_trader.data.messages cimport SubscribeIndexPrices
from nautilus_trader.data.messages cimport SubscribeInstrument
from nautilus_trader.data.messages cimport SubscribeInstrumentClose
from nautilus_trader.data.messages cimport SubscribeInstruments
from nautilus_trader.data.messages cimport SubscribeInstrumentStatus
from nautilus_trader.data.messages cimport SubscribeMarkPrices
from nautilus_trader.data.messages cimport SubscribeOrderBook
from nautilus_trader.data.messages cimport SubscribeQuoteTicks
from nautilus_trader.data.messages cimport SubscribeTradeTicks
from nautilus_trader.data.messages cimport UnsubscribeBars
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.data.messages cimport UnsubscribeFundingRates
from nautilus_trader.data.messages cimport UnsubscribeIndexPrices
from nautilus_trader.data.messages cimport UnsubscribeInstrument
from nautilus_trader.data.messages cimport UnsubscribeInstrumentClose
from nautilus_trader.data.messages cimport UnsubscribeInstruments
from nautilus_trader.data.messages cimport UnsubscribeInstrumentStatus
from nautilus_trader.data.messages cimport UnsubscribeMarkPrices
from nautilus_trader.data.messages cimport UnsubscribeOrderBook
from nautilus_trader.data.messages cimport UnsubscribeQuoteTicks
from nautilus_trader.data.messages cimport UnsubscribeTradeTicks
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.option_spread cimport OptionSpread


cdef class BacktestDataClient(DataClient):
    """
    Provides an implementation of `DataClient` for backtesting.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    config : NautilusConfig, optional
        The configuration for the instance.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        config: NautilusConfig | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            venue=Venue(client_id.to_str()),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self.is_connected = False

    cpdef void _start(self):
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe(self, SubscribeData command):
        Condition.not_none(command.data_type, "data_type")

        self._add_subscription(command.data_type)
        # Do nothing else for backtest

    cpdef void unsubscribe(self, UnsubscribeData command):
        Condition.not_none(command.data_type, "data_type")

        self._remove_subscription(command.data_type)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, RequestData request):
        Condition.not_none(request.data_type, "data_type")
        # Do nothing else for backtest


cdef class BacktestMarketDataClient(MarketDataClient):
    """
    Provides an implementation of `MarketDataClient` for backtesting.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
    ):
        super().__init__(
            client_id=client_id,
            venue=Venue(client_id.to_str()),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self.is_connected = False
        self._spread_quote_aggregators = {} # type: dict[InstrumentId, SpreadQuoteAggregator]

    cpdef void _start(self):
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")

        # Stop all spread quote aggregators
        if self._spread_quote_aggregators is not None:
            for aggregator in self._spread_quote_aggregators.values():
                aggregator.stop()

        self.is_connected = False
        self._log.info(f"Disconnected")

    cpdef void _reset(self):
        # Stop and clear all spread quote aggregators
        for aggregator in self._spread_quote_aggregators.values():
            aggregator.stop()

        self._spread_quote_aggregators.clear()

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe(self, SubscribeData command):
        Condition.not_none(command.data_type, "data_type")

        if command.instrument_id and not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for {command.data_type} data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription(command.data_type)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void unsubscribe(self, UnsubscribeData command):
        Condition.not_none(command.data_type, "data_type")

        self._remove_subscription(command.data_type)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void subscribe_instruments(self, SubscribeInstruments command):
        cdef Instrument instrument

        for instrument in self._cache.instruments(Venue(self.id.value)):
            subscribe = SubscribeInstrument(
                instrument_id=instrument.id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params,
            )
            self.subscribe_instrument(subscribe)

        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void subscribe_instrument(self, SubscribeInstrument command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `Instrument` data",
            )
            return

        self._add_subscription_instrument(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_deltas(self, SubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `OrderBookDelta` data, "
                "no data has been loaded for this instrument",
            )
            return

        self._add_subscription_order_book_deltas(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_snapshots(self, SubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `OrderBook` data, "
                "no data has been loaded for this instrument.",
            )
            return

        self._add_subscription_order_book_snapshots(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_depth(self, SubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `OrderBookDepth10` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_order_book_snapshots(command.instrument_id)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void subscribe_quote_ticks(self, SubscribeQuoteTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        # Handle spread instruments
        if command.instrument_id.is_spread():
            self._start_spread_quote_aggregator(command)
            return

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `QuoteTick` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_quote_ticks(command.instrument_id)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void subscribe_trade_ticks(self, SubscribeTradeTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `TradeTick` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_trade_ticks(command.instrument_id)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void subscribe_mark_prices(self, SubscribeMarkPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `MarkPriceUpdate` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_mark_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_index_prices(self, SubscribeIndexPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `IndexPriceUpdate` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_index_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_funding_rates(self, SubscribeFundingRates command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `FundingRateUpdate` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_funding_rates(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_bars(self, SubscribeBars command):
        Condition.not_none(command.bar_type, "bar_type")

        if not self._cache.instrument(command.bar_type.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.bar_type.instrument_id} to subscribe for `Bar` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_bars(command.bar_type)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void subscribe_instrument_status(self, SubscribeInstrumentStatus command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._add_subscription_instrument_status(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_close(self, SubscribeInstrumentClose command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._add_subscription_instrument_close(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instruments(self, UnsubscribeInstruments command):
        self._subscriptions_instrument.clear()
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void unsubscribe_instrument(self, UnsubscribeInstrument command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_instrument(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_deltas(self, UnsubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_order_book_deltas(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_snapshots(self, UnsubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_order_book_snapshots(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_depth(self, UnsubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_order_book_snapshots(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_quote_ticks(self, UnsubscribeQuoteTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        # Handle spread instruments
        if command.instrument_id.is_spread():
            self._stop_spread_quote_aggregator(command)
            return

        self._remove_subscription_quote_ticks(command.instrument_id)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void unsubscribe_trade_ticks(self, UnsubscribeTradeTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_trade_ticks(command.instrument_id)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void unsubscribe_mark_prices(self, UnsubscribeMarkPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_mark_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_index_prices(self, UnsubscribeIndexPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_index_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_funding_rates(self, UnsubscribeFundingRates command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_funding_rates(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_bars(self, UnsubscribeBars command):
        Condition.not_none(command.bar_type, "bar_type")

        self._remove_subscription_bars(command.bar_type)
        self._msgbus.send(endpoint="BacktestEngine.execute", msg=command)

    cpdef void unsubscribe_instrument_status(self, UnsubscribeInstrumentStatus command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_instrument_status(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument_close(self, UnsubscribeInstrumentClose command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_instrument_close(command.instrument_id)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request_instrument(self, RequestInstrument request):
        cdef Instrument instrument = self._cache.instrument(request.instrument_id)

        if instrument is None:
            # Check if this is a spread instrument - we're already in backtest context
            if request.instrument_id.is_spread():
                # Create OptionSpread from component instruments
                instrument = self._create_option_spread_from_components(request.instrument_id)

                if instrument is not None:
                    self._cache.add_instrument(instrument)
                    self._log.info(f"Created OptionSpread instrument {request.instrument_id} from components")
                else:
                    self._log.error(f"Failed to create OptionSpread instrument {request.instrument_id} from components")
                    return
            else:
                self._log.error(f"Cannot find instrument for {request.instrument_id}")
                return

        self._handle_instrument(instrument, request.id, request.start, request.end, request.params)

    cdef Instrument _create_option_spread_from_components(self, InstrumentId spread_instrument_id):
        min_expiration_ns = 0

        try:
            # Parse component instruments from spread ID
            components = spread_instrument_id.to_list()

            if not components:
                self._log.error(f"No components found in spread instrument ID {spread_instrument_id}")
                return None

            # Get the first component instrument to use as template
            first_component_id = components[0][0]
            first_component = self._cache.instrument(first_component_id)

            if first_component is None:
                self._log.error(f"Cannot find first component instrument {first_component_id} for spread {spread_instrument_id}")
                return None

            # Validate all components exist and find minimum expiration
            for component_id, ratio in components:
                component = self._cache.instrument(component_id)

                if component is None:
                    self._log.error(f"Cannot find component instrument {component_id} for spread {spread_instrument_id}")
                    return None

                if min_expiration_ns == 0 or component.expiration_ns < min_expiration_ns:
                    min_expiration_ns = component.expiration_ns

            # Create timestamp
            ts_event = self._clock.timestamp_ns()

            # Create the OptionSpread instrument
            return OptionSpread(
                instrument_id=spread_instrument_id,
                raw_symbol=Symbol(spread_instrument_id.symbol.value),
                asset_class=first_component.asset_class,
                currency=first_component.quote_currency,
                price_precision=first_component.price_precision,
                price_increment=first_component.price_increment,
                multiplier=first_component.multiplier,
                lot_size=first_component.lot_size,
                underlying="",
                strategy_type="SPREAD",
                activation_ns=0,
                expiration_ns=min_expiration_ns,
                ts_event=ts_event,
                ts_init=ts_event,
                margin_init=first_component.margin_init,
                margin_maint=first_component.margin_maint,
                maker_fee=first_component.maker_fee,
                taker_fee=first_component.taker_fee,
                exchange=first_component.exchange,
                tick_scheme_name=first_component.tick_scheme_name,
            )
        except Exception as e:
            self._log.error(f"Failed to create OptionSpread from components: {e}")
            return

    cpdef void request_instruments(self, RequestInstruments request):
        cdef list instruments = self._cache.instruments(request.venue)

        if not instruments:
            self._log.error(f"Cannot find instruments")
            return

        self._handle_instruments(request.venue, instruments, request.id, request.start, request.end, request.params)

    cpdef void request_order_book_snapshot(self, RequestOrderBookSnapshot request):
        # Do nothing else for backtest
        pass

    cpdef void request_quote_ticks(self, RequestQuoteTicks request):
        # Do nothing else for backtest
        pass

    cpdef void request_trade_ticks(self, RequestTradeTicks request):
        # Do nothing else for backtest
        pass

    cpdef void request_bars(self, RequestBars request):
        # Do nothing else for backtest
        pass

# -- SPREAD QUOTE AGGREGATORS --------------------------------------------------------------------

    cpdef void _start_spread_quote_aggregator(self, SubscribeQuoteTicks command):
        spread_instrument_id = command.instrument_id

        # Ensure the dictionary is initialized
        if self._spread_quote_aggregators is None:
            self._spread_quote_aggregators = {}

        if spread_instrument_id in self._spread_quote_aggregators:
            return

        update_interval_seconds = command.params.get("update_interval_seconds", 60)
        aggregator = SpreadQuoteAggregator(
            spread_instrument_id=spread_instrument_id,
            handler=self._handle_spread_quote,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            update_interval_seconds=update_interval_seconds,
        )
        self._spread_quote_aggregators[spread_instrument_id] = aggregator

        # Subscribe to quotes for component instruments
        components = spread_instrument_id.to_list()

        for component_id, _ in components:
            subscribe = SubscribeQuoteTicks(
                instrument_id=component_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params,
            )

            # Send command to message bus for normal treatment
            self._msgbus.send(endpoint="DataEngine.execute", msg=subscribe)

    cpdef void _stop_spread_quote_aggregator(self, UnsubscribeQuoteTicks command):
        spread_instrument_id = command.instrument_id

        # Ensure the dictionary is initialized
        if self._spread_quote_aggregators is None:
            self._spread_quote_aggregators = {}
            return

        aggregator = self._spread_quote_aggregators.get(spread_instrument_id)

        if aggregator is None:
            self._log.warning(
                f"Cannot stop spread quote aggregator: no aggregator found for {spread_instrument_id}",
            )
            return

        aggregator.stop()

        # Unsubscribe from component instruments
        components = spread_instrument_id.to_list()

        for component_id, _ in components:
            unsubscribe = UnsubscribeQuoteTicks(
                instrument_id=component_id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params,
            )

            # Send command to message bus for normal treatment
            self._msgbus.send(endpoint="DataEngine.execute", msg=unsubscribe)

        del self._spread_quote_aggregators[spread_instrument_id]

    cdef void _handle_spread_quote(self, quote):
        """
        Handle a spread quote generated by the aggregator.

        Parameters
        ----------
        quote : QuoteTick
            The spread quote to handle.
        """
        # Send the quote to the data engine for processing
        self._msgbus.send(endpoint="DataEngine.process", msg=quote)

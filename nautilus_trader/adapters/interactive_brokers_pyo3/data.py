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
Provides a PyO3-based data client for Interactive Brokers.

This adapter uses PyO3 bindings to call the Rust implementation of the Interactive
Brokers adapter, providing the same API as the Python adapter but with Rust performance.

"""

from __future__ import annotations

import asyncio
from types import ModuleType
from typing import TYPE_CHECKING
from typing import Any

from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersDataClientConfig,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeOptionGreeks
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeOptionGreeks
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import OptionGreeks
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments.currency_pair import CurrencyPair


if TYPE_CHECKING:
    from nautilus_trader.adapters.interactive_brokers_pyo3.providers import (
        InteractiveBrokersInstrumentProvider,
    )

nautilus_pyo3: ModuleType | None
try:
    import nautilus_trader.core.nautilus_pyo3 as nautilus_pyo3
except ImportError:
    nautilus_pyo3 = None

try:
    from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
        InteractiveBrokersDataClient as RustInteractiveBrokersDataClient,
    )
except ImportError:
    RustInteractiveBrokersDataClient = None


def _to_pyo3_instrument_id(value: Any) -> Any:
    if nautilus_pyo3 is None or value is None:
        return value
    raw_value = getattr(value, "value", None)
    return nautilus_pyo3.InstrumentId.from_str(raw_value or str(value))


def _to_pyo3_bar_type(value: Any) -> Any:
    if nautilus_pyo3 is None or value is None:
        return value
    return nautilus_pyo3.BarType.from_str(str(value))


def _to_unix_nanos(value: Any) -> int | None:
    if value is None:
        return None

    as_u64 = getattr(value, "as_u64", None)
    if callable(as_u64):
        return int(as_u64())

    raw_value = getattr(value, "value", None)
    if raw_value is not None:
        return int(raw_value)

    return int(value)


class InteractiveBrokersDataClient(LiveMarketDataClient):
    """
    Provides a PyO3-based data client for Interactive Brokers.

    This class wraps the Rust implementation via PyO3 bindings, providing
    the same API as the Python adapter but using the Rust implementation.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.
    config : InteractiveBrokersDataClientConfig
        Configuration for the client.
    name : str, optional
        The custom client ID.

    Raises
    ------
    ImportError
        If the PyO3 bindings are not available.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: InteractiveBrokersInstrumentProvider,
        config: InteractiveBrokersDataClientConfig,
        name: str | None = None,
    ) -> None:
        if RustInteractiveBrokersDataClient is None:
            raise ImportError(
                "PyO3 bindings for Interactive Brokers are not available. "
                "Please ensure the extension module is built with the 'extension-module' feature.",
            )

        # Initialize the Rust client via PyO3
        self._pending_requests: dict[str, Any] = {}
        self._rust_client = RustInteractiveBrokersDataClient(
            msgbus,
            cache,
            clock,
            instrument_provider._rust_provider,
            config,
        )
        instrument_provider._attach_loader(self._rust_client)
        self._ib_instrument_provider = instrument_provider
        self._ib_config = config

        # Initialize the Python base class
        super().__init__(
            loop=loop,
            client_id=ClientId(name or self._rust_client.client_id.value),
            venue=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=None,
        )

        if hasattr(self._rust_client, "set_event_callback"):
            self._rust_client.set_event_callback(self._on_rust_event)

    async def _connect(self):
        """
        Connect the client.
        """
        self._rust_client.connect()
        await self._instrument_provider.initialize()
        for instrument in self._instrument_provider.list_all():
            self._handle_data(instrument)

    async def _disconnect(self):
        """
        Disconnect the client.
        """
        self._rust_client.disconnect()

    def _handle_data(self, data: Any) -> None:
        """
        Handle incoming data from the Rust client.
        """
        super()._handle_data(data)

    def _on_rust_event(self, kind: str, correlation_id: str | None, payload: Any) -> None:
        self._loop.call_soon_threadsafe(
            self._handle_rust_event,
            kind,
            correlation_id,
            payload,
        )

    def _handle_rust_event(self, kind: str, correlation_id: str | None, payload: Any) -> None:  # noqa: C901
        if kind == "quote":
            self._handle_data(QuoteTick.from_pyo3(payload))
            return
        if kind == "trade":
            self._handle_data(TradeTick.from_pyo3(payload))
            return
        if kind == "bar":
            self._handle_data(Bar.from_pyo3(payload))
            return
        if kind == "delta":
            self._handle_data(OrderBookDelta.from_pyo3(payload))
            return
        if kind == "index_price":
            self._handle_data(IndexPriceUpdate.from_pyo3(payload))
            return
        if kind == "option_greeks":
            self._handle_data(OptionGreeks.from_pyo3(payload))
            return
        if kind == "instrument":
            instrument = transform_instrument_from_pyo3(payload)
            if instrument is not None:
                self._handle_data(instrument)
            return

        if correlation_id is None:
            self._log.warning(f"Received IB PyO3 callback without correlation_id for kind={kind}")
            return

        request = self._pending_requests.pop(correlation_id, None)
        if request is None:
            self._log.warning(
                f"Received unmatched IB PyO3 data callback kind={kind} correlation_id={correlation_id}",
            )
            return

        if kind == "instrument_response":
            instrument = transform_instrument_from_pyo3(payload)
            if instrument is not None:
                self._handle_data(instrument)
                self._handle_instrument(
                    instrument,
                    request.id,
                    request.start,
                    request.end,
                    request.params,
                )
            return

        if kind == "instruments_response":
            instruments = [transform_instrument_from_pyo3(item) for item in payload]
            instruments = [instrument for instrument in instruments if instrument is not None]
            self._handle_instruments(
                venue=request.venue,
                instruments=instruments,
                correlation_id=request.id,
                start=request.start,
                end=request.end,
                params=request.params,
            )
            return

        if kind == "quotes_response":
            self._handle_quote_ticks(
                request.instrument_id,
                QuoteTick.from_pyo3_list(payload),
                request.id,
                request.start,
                request.end,
                request.params,
            )
            return

        if kind == "trades_response":
            self._handle_trade_ticks(
                request.instrument_id,
                TradeTick.from_pyo3_list(payload),
                request.id,
                request.start,
                request.end,
                request.params,
            )
            return

        if kind == "bars_response":
            self._handle_bars(
                request.bar_type,
                Bar.from_pyo3_list(payload),
                request.id,
                request.start,
                request.end,
                request.params,
            )
            return

        self._log.warning(f"Unhandled IB PyO3 data callback kind={kind}")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to order book deltas.
        """
        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot subscribe to order book deltas for {command.instrument_id}: instrument not found",
            )
            return

        depth = command.depth if command.depth else 20
        params = {}
        if "is_smart_depth" in command.params:
            params["is_smart_depth"] = str(command.params["is_smart_depth"])

        self._rust_client.subscribe_book_deltas(
            _to_pyo3_instrument_id(command.instrument_id),
            depth if depth != 20 else None,
            params if params else None,
        )

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        """
        Subscribe to quote ticks.
        """
        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot subscribe to quotes for {command.instrument_id}: instrument not found",
            )
            return

        params = {}
        if "batch_quotes" in command.params:
            params["batch_quotes"] = str(command.params["batch_quotes"])

        self._rust_client.subscribe_quotes(
            _to_pyo3_instrument_id(command.instrument_id),
            params if params else None,
        )

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        """
        Subscribe to index prices.
        """
        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot subscribe to index prices for {command.instrument_id}: instrument not found",
            )
            return

        self._rust_client.subscribe_index_prices(_to_pyo3_instrument_id(command.instrument_id))

    async def _subscribe_option_greeks(self, command: SubscribeOptionGreeks) -> None:
        """
        Subscribe to option greeks.
        """
        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot subscribe to option greeks for {command.instrument_id}: instrument not found",
            )
            return

        self._rust_client.subscribe_option_greeks(
            _to_pyo3_instrument_id(command.instrument_id),
        )

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        """
        Subscribe to trade ticks.
        """
        instrument = self._cache.instrument(command.instrument_id)
        if not instrument:
            self._log.error(
                f"Cannot subscribe to trades for {command.instrument_id}: instrument not found",
            )
            return

        if isinstance(instrument, CurrencyPair):
            self._log.error(
                "Interactive Brokers does not support trades for CurrencyPair instruments",
            )
            return

        self._rust_client.subscribe_trades(_to_pyo3_instrument_id(command.instrument_id))

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        """
        Subscribe to bars.
        """
        if not self._cache.instrument(command.bar_type.instrument_id):
            self._log.error(
                f"Cannot subscribe to bars for {command.bar_type.instrument_id}: instrument not found",
            )
            return

        params = None
        command_params = getattr(command, "params", None)
        if command_params:
            params = {key: str(value) for key, value in command_params.items() if value is not None}

        pyo3_bar_type = _to_pyo3_bar_type(command.bar_type)
        if params is None:
            self._rust_client.subscribe_bars(pyo3_bar_type)
        else:
            self._rust_client.subscribe_bars(pyo3_bar_type, params)

    async def _subscribe_instrument_status(self, command: Any) -> None:
        """
        Subscribe to instrument status (handled via orderbook).
        """
        # Subscribed as part of orderbook

    async def _subscribe_instrument_close(self, command: Any) -> None:
        """
        Subscribe to instrument close (handled via orderbook).
        """
        # Subscribed as part of orderbook

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        """
        Unsubscribe from order book deltas.
        """
        self._rust_client.unsubscribe_book_deltas(_to_pyo3_instrument_id(command.instrument_id))

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        """
        Unsubscribe from quote ticks.
        """
        self._rust_client.unsubscribe_quotes(_to_pyo3_instrument_id(command.instrument_id))

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        """
        Unsubscribe from index prices.
        """
        self._rust_client.unsubscribe_index_prices(_to_pyo3_instrument_id(command.instrument_id))

    async def _unsubscribe_option_greeks(self, command: UnsubscribeOptionGreeks) -> None:
        """
        Unsubscribe from option greeks.
        """
        self._rust_client.unsubscribe_option_greeks(
            _to_pyo3_instrument_id(command.instrument_id),
        )

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        """
        Unsubscribe from trade ticks.
        """
        self._rust_client.unsubscribe_trades(_to_pyo3_instrument_id(command.instrument_id))

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        """
        Unsubscribe from bars.
        """
        self._rust_client.unsubscribe_bars(_to_pyo3_bar_type(command.bar_type))

    async def _unsubscribe_instrument_status(self, command: Any) -> None:
        """
        Unsubscribe from instrument status (handled via orderbook).
        """
        # Subscribed as part of orderbook

    async def _unsubscribe_instrument_close(self, command: Any) -> None:
        """
        Unsubscribe from instrument close (handled via orderbook).
        """
        # Subscribed as part of orderbook

    async def _request_instrument(self, request: RequestInstrument) -> None:
        """
        Request a single instrument.
        """
        if request.start is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        params = request.params or {}

        # Load instrument via provider and handle it.
        await self._ib_instrument_provider.load_with_return_async(
            request.instrument_id,
            params,
        )

        if instrument := self._ib_instrument_provider.find(request.instrument_id):
            self._handle_data(instrument)
        else:
            self._log.warning(f"Instrument for {request.instrument_id} not available")
            return

        self._handle_instrument(instrument, request.id, request.start, request.end, request.params)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        """
        Request multiple instruments.
        """
        loaded_instrument_ids: list = []

        if "ib_contracts" in request.params:
            loaded_instrument_ids = await self._ib_instrument_provider.load_ids_with_return_async(
                request.params["ib_contracts"],
                request.params,
            )
            loaded_instruments = []

            if loaded_instrument_ids:
                for instrument_id in loaded_instrument_ids:
                    instrument = self._cache.instrument(instrument_id)

                    if instrument is None:
                        instrument = self._ib_instrument_provider.find(instrument_id)
                        if instrument is not None:
                            self._handle_data(instrument)

                    if instrument:
                        loaded_instruments.append(instrument)
                    else:
                        self._log.warning(
                            f"Instrument {instrument_id} not found in cache after loading",
                        )
            else:
                self._log.warning("No instrument IDs were returned from load_ids_async")

            self._handle_instruments(
                venue=request.venue,
                instruments=loaded_instruments,
                correlation_id=request.id,
                start=request.start,
                end=request.end,
                params=request.params,
            )
            return

        instruments = self._cache.instruments()
        instrument_ids = [instrument.id for instrument in instruments]
        await self._ib_instrument_provider.load_ids_with_return_async(
            instrument_ids,
            request.params,
        )
        self._handle_instruments(
            venue=request.venue,
            instruments=[],
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        """
        Request historical quote ticks.
        """
        if not self._cache.instrument(request.instrument_id):
            self._log.error(
                f"Cannot request quotes for {request.instrument_id}, instrument not found",
            )
            return

        correlation_id = request.id.value
        self._pending_requests[correlation_id] = request
        try:
            self._rust_client.request_quotes(
                _to_pyo3_instrument_id(request.instrument_id),
                request.limit,
                _to_unix_nanos(request.start),
                _to_unix_nanos(request.end),
                str(request.id),
            )
        except Exception:
            self._pending_requests.pop(correlation_id, None)
            raise

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        """
        Request historical trade ticks.
        """
        if not self._cache.instrument(request.instrument_id):
            self._log.error(
                f"Cannot request trades for {request.instrument_id}, instrument not found",
            )
            return

        correlation_id = request.id.value
        self._pending_requests[correlation_id] = request
        try:
            self._rust_client.request_trades(
                _to_pyo3_instrument_id(request.instrument_id),
                request.limit,
                _to_unix_nanos(request.start),
                _to_unix_nanos(request.end),
                str(request.id),
            )
        except Exception:
            self._pending_requests.pop(correlation_id, None)
            raise

    async def _request_bars(self, request: RequestBars) -> None:
        """
        Request historical bars.
        """
        if not self._cache.instrument(request.bar_type.instrument_id):
            self._log.error(
                f"Cannot request bars for {request.bar_type.instrument_id}, instrument not found",
            )
            return

        correlation_id = request.id.value
        self._pending_requests[correlation_id] = request
        try:
            self._rust_client.request_bars(
                _to_pyo3_bar_type(request.bar_type),
                request.limit,
                _to_unix_nanos(request.start),
                _to_unix_nanos(request.end),
                str(request.id),
            )
        except Exception:
            self._pending_requests.pop(correlation_id, None)
            raise

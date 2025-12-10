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

from __future__ import annotations

import asyncio
from typing import Any

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class LighterDataClient(LiveMarketDataClient):
    """
    Live market data client for Lighter Exchange.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: Any,  # nautilus_pyo3.lighter.LighterHttpClient
        ws_client: Any,  # nautilus_pyo3.lighter.LighterWebSocketClient
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: LighterInstrumentProvider,
        config: LighterDataClientConfig,
        name: str,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or LIGHTER_VENUE.value),
            venue=LIGHTER_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._http_client = http_client
        self._ws_client = ws_client
        self._config = config
        self._pyo3_instruments: dict[str, Any] = {}
        self._last_book_offsets: dict[str, int] = {}

        self._log.info(f"config.testnet={config.testnet}", LogColor.BLUE)
        self._log.info(f"{config.base_url_ws=}", LogColor.BLUE)
        self._log.info(f"{config.base_url_http=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)

    @property
    def instrument_provider(self) -> LighterInstrumentProvider:
        return self._instrument_provider  # type: ignore[return-value]

    async def _connect(self) -> None:
        await self.instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        self._pyo3_instruments = {}
        for inst in instruments_pyo3:
            try:
                inst_id = inst.id()
                key = getattr(inst_id, "value", str(inst_id))
                self._pyo3_instruments[key] = inst
            except Exception:  # pragma: no cover - defensive
                continue

        await self._ws_client.connect(instruments_pyo3, self._handle_msg)
        await self._ws_client.wait_until_active(timeout_ms=10_000)

    async def _disconnect(self) -> None:
        await self._ws_client.close()

    # ---------------------------------------------------------------------
    # Message handling
    # ---------------------------------------------------------------------

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                data = capsule_to_data(msg)
                if isinstance(data, OrderBookDeltas):
                    self._handle_order_book_deltas(data)
                else:
                    self._handle_data(data)
            elif isinstance(msg, nautilus_pyo3.FundingRateUpdate):
                data = FundingRateUpdate.from_pyo3(msg)
                self._handle_data(data)
            else:
                self._log.warning(f"Unhandled websocket message type: {type(msg)}")
        except Exception as exc:  # pragma: no cover - defensive
            self._log.exception("Error handling websocket message", exc)

    def _handle_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        instrument_id = deltas.instrument_id
        key = getattr(instrument_id, "value", str(instrument_id))
        sequence = getattr(deltas, "sequence", None)

        if sequence is not None:
            last = self._last_book_offsets.get(key)
            if last is not None and sequence != last + 1:
                self._log.warning(
                    f"Detected order book gap for {instrument_id}: last={last}, new={sequence} "
                    "- requesting fresh snapshot",
                )
                self._loop.create_task(self._resync_order_book(instrument_id))
                return

            self._last_book_offsets[key] = sequence

        self._handle_data(deltas)

    async def _resync_order_book(self, instrument_id: InstrumentId) -> None:
        key = getattr(instrument_id, "value", str(instrument_id))
        instrument = self._pyo3_instruments.get(key)
        if instrument is None:
            self._log.warning(
                f"Cannot resync order book for {instrument_id}: missing PyO3 instrument",
            )
            return

        try:
            snapshot = await self._http_client.get_order_book_snapshot(instrument)
            if isinstance(snapshot, OrderBookDeltas):
                self._handle_data(snapshot)
                self._last_book_offsets.pop(key, None)
            elif nautilus_pyo3.is_pycapsule(snapshot):  # pragma: no cover
                self._handle_data(capsule_to_data(snapshot))
                self._last_book_offsets.pop(key, None)
            else:  # pragma: no cover - defensive
                self._log.warning(
                    "Unexpected snapshot payload type during resync "
                    f"for {instrument_id}: {type(snapshot)}",
                )
        except Exception as exc:  # pragma: no cover - defensive
            self._log.exception(f"Failed resync for {instrument_id}", exc)

    # ---------------------------------------------------------------------
    # Subscription handlers
    # ---------------------------------------------------------------------

    async def _subscribe_order_book_deltas(self, command) -> None:
        await self._subscribe_order_book(command)

    async def _subscribe_order_book_snapshots(self, command) -> None:
        await self._subscribe_order_book(command)

    async def _unsubscribe_order_book_deltas(self, command) -> None:
        await self._unsubscribe_order_book(command)

    async def _unsubscribe_order_book_snapshots(self, command) -> None:
        await self._unsubscribe_order_book(command)

    async def _subscribe_order_book(self, command) -> None:
        market_index = self._market_index_for(command.instrument_id)
        if market_index is None:
            return
        await self._ws_client.subscribe_order_book(market_index)

    async def _unsubscribe_order_book(self, command) -> None:
        market_index = self._market_index_for(command.instrument_id)
        if market_index is None:
            return
        await self._ws_client.unsubscribe_order_book(market_index)

    async def _subscribe_trade_ticks(self, command) -> None:
        market_index = self._market_index_for(command.instrument_id)
        if market_index is None:
            return
        await self._ws_client.subscribe_trades(market_index)

    async def _unsubscribe_trade_ticks(self, command) -> None:
        market_index = self._market_index_for(command.instrument_id)
        if market_index is None:
            return
        await self._ws_client.unsubscribe_trades(market_index)

    async def _subscribe_mark_prices(self, command) -> None:
        market_index = self._market_index_for(command.instrument_id)
        if market_index is None:
            return
        await self._ws_client.subscribe_market_stats(market_index)

    async def _subscribe_index_prices(self, command) -> None:
        await self._subscribe_mark_prices(command)

    async def _subscribe_funding_rates(self, command) -> None:
        await self._subscribe_mark_prices(command)

    async def _unsubscribe_mark_prices(self, command) -> None:
        market_index = self._market_index_for(command.instrument_id)
        if market_index is None:
            return
        await self._ws_client.unsubscribe_market_stats(market_index)

    async def _unsubscribe_index_prices(self, command) -> None:
        await self._unsubscribe_mark_prices(command)

    async def _unsubscribe_funding_rates(self, command) -> None:
        await self._unsubscribe_mark_prices(command)

    async def _subscribe_quote_ticks(self, command) -> None:
        self._log.debug("Quote ticks derived from order book; no direct subscription required")

    async def _unsubscribe_quote_ticks(self, command) -> None:
        self._log.debug("Quote ticks derived from order book; no direct subscription required")

    async def _subscribe_instrument(self, command) -> None:  # pragma: no cover - unused
        self._log.info(f"Subscribed instrument {command.instrument_id}")

    async def _subscribe_instruments(self, command) -> None:  # pragma: no cover - unused
        self._log.info("Subscribed instruments")

    async def _unsubscribe_instrument(self, command) -> None:  # pragma: no cover - unused
        self._log.info(f"Unsubscribed instrument {command.instrument_id}")

    async def _unsubscribe_instruments(self, command) -> None:  # pragma: no cover - unused
        self._log.info("Unsubscribed instruments")

    # ---------------------------------------------------------------------
    # Requests
    # ---------------------------------------------------------------------

    async def _request_order_book_snapshot(self, request) -> None:
        instrument_id = request.instrument_id
        key = getattr(instrument_id, "value", str(instrument_id))
        instrument = self._pyo3_instruments.get(key)
        if instrument is None:
            self._log.warning(f"Missing instrument for snapshot request: {instrument_id}")
            return

        try:
            snapshot = await self._http_client.get_order_book_snapshot(instrument)
            if isinstance(snapshot, OrderBookDeltas):
                self._handle_data(snapshot)
                self._last_book_offsets.pop(key, None)
            elif nautilus_pyo3.is_pycapsule(snapshot):  # pragma: no cover
                self._handle_data(capsule_to_data(snapshot))
                self._last_book_offsets.pop(key, None)
            else:  # pragma: no cover - defensive
                self._log.warning(
                    f"Unexpected snapshot payload for {instrument_id}: {type(snapshot)}",
                )
        except Exception as exc:  # pragma: no cover - defensive
            self._log.exception(f"Failed order book snapshot request for {instrument_id}", exc)

    async def _request_trade_ticks(self, request) -> None:
        self._log.warning("Historical trades not supported for Lighter")

    async def _request_quote_ticks(self, request) -> None:
        self._log.warning("Historical quotes not supported for Lighter")

    async def _request_bars(self, request) -> None:
        self._log.warning("Historical bars not implemented for Lighter")

    async def _request_instrument(self, request) -> None:
        instrument = self.instrument_provider.find(request.instrument_id)
        if instrument:
            self._handle_data(instrument)

    async def _request_instruments(self, request) -> None:
        for instrument_id in request.instrument_ids:
            instrument = self.instrument_provider.find(instrument_id)
            if instrument:
                self._handle_data(instrument)

    async def _request_instrument_ids(self, request) -> None:  # pragma: no cover - unused
        self._log.debug("Instrument ID request not supported for Lighter")

    async def _request_instrument_definitions(self, request) -> None:  # pragma: no cover
        self._log.debug("Instrument definition request not supported for Lighter")

    async def _request_instrument_status(self, request) -> None:  # pragma: no cover
        self._log.debug("Instrument status request not supported for Lighter")

    # ---------------------------------------------------------------------
    # Helpers
    # ---------------------------------------------------------------------

    def _cache_instruments(self) -> None:
        for instrument in self.instrument_provider.get_all().values():
            self._cache.add_instrument(instrument)
        for currency in self.instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)

    def _market_index_for(self, instrument_id: InstrumentId) -> int | None:
        PyCondition.not_none(instrument_id, "instrument_id")
        market_index = self.instrument_provider.market_index_for(instrument_id)
        if market_index is None:
            self._log.warning(
                f"Missing market index for {instrument_id}, skipping subscription",
            )
        return market_index

    def _generate_request_id(self) -> UUID4:
        return UUID4()

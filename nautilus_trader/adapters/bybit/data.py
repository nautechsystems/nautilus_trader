import asyncio
from typing import Optional

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.websocket.client import BybitWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.msgbus.bus import MessageBus


class BybitDataClient(LiveMarketDataClient):
    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InstrumentProvider,
        account_type: BybitAccountType,
        base_url_ws: str,
        config: BybitDataClientConfig,
    ) -> None:
        self._enum_parser = BybitEnumParser()
        super().__init__(
            loop=loop,
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        # websocket API
        self._ws_client = BybitWebsocketClient(
            clock=clock,
            logger=logger,
            handler=self._handle_ws_message,
            base_url=base_url_ws,
        )

        self._update_instrument_interval: int = 60 * 60  # Once per hour (hardcode)
        self._update_instruments_task: Optional[asyncio.Task] = None

    async def _connect(self) -> None:
        self._log.info("Initializing instruments...")
        await self._instrument_provider.initialize()

        self._send_all_instruments_to_data_engine()
        self._update_instruments_task = self.create_task(self._update_instruments())

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    async def _update_instruments(self) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled `update_instruments` to run in "
                    f"{self._update_instrument_interval}s.",
                )
                await asyncio.sleep(self._update_instrument_interval)
                await self._instrument_provider.load_all_async()
                self._send_all_instruments_to_data_engine()
        except asyncio.CancelledError:
            self._log.debug("`update_instruments` task was canceled.")

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        symbol = BybitSymbol(instrument_id.symbol.value)
        await self._ws_client.subscribe_trades(symbol)

    def _handle_ws_message(self, raw: bytes) -> None:
        print("tu smo")

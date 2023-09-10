import asyncio
from typing import Optional

import pandas as pd

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.adapters.bybit.schemas.position import BybitPositionStruct
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.orders import Order
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class BybitExecutionClient(LiveExecutionClient):
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
        config: BybitExecClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH if account_type.is_spot else AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )
        self._bybit_account_type = account_type
        self._enum_parser = BybitEnumParser()
        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}

        # Http API
        self._http_account = BybitAccountHttpAPI(
            client=client,
            clock=clock,
            account_type=account_type,
        )

    async def _connect(self) -> None:
        # Initialize instrument provider
        await self._instrument_provider.initialize()
        # Update account state
        await self._update_account_state()

    async def generate_order_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self._log.info("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []
        try:
            symbol = instrument_id.symbol.value if instrument_id is not None else None
            # active_symbols = self._get_cache_active_symbols()
            # active_symbols.update(await self._get_active_position_symbols(symbol))
            open_orders = await self._http_account.query_open_orders(symbol)
            for order in open_orders:
                report = order.parse_to_order_status_report(
                    account_id=self.account_id,
                    instrument_id=self._get_cached_instrument_id(order.symbol),
                    report_id=UUID4(),
                    enum_parser=self._enum_parser,
                    ts_init=self._clock.timestamp_ns(),
                )
                reports.append(report)
            self._log.debug(f"Received {report}")
        except BybitError as e:
            self._log.error(f"Failed to generate OrderStatusReports: {e}")
        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} OrderStatusReport{plural}.")
        return reports

    async def generate_trade_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[TradeReport]:
        self._log.info("Requesting TradeReports...")
        return []

    async def generate_position_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[PositionStatusReport]:
        self._log.info("Requesting PositionStatusReports...")
        return []

    def _get_cache_active_symbols(self) -> set[str]:
        # check in cache for all active orders
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: set[str] = set()
        for order in open_orders:
            active_symbols.add(BybitSymbol(order.instrument_id.symbol.value))
        for position in open_positions:
            active_symbols.add(BybitSymbol(position.instrument_id.symbol.value))
        return active_symbols

    def _get_cached_instrument_id(self, symbol: str) -> Optional[InstrumentId]:
        # parse instrument id
        nautilus_symbol: str = BybitSymbol(symbol).parse_as_nautilus(self._bybit_account_type)
        instrument_id: InstrumentId = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), BYBIT_VENUE)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id

    async def _get_active_position_symbols(self, symbol: Optional[str]) -> set[str]:
        active_symbols: set[str] = set()
        positions: list[BybitPositionStruct]
        bybit_positions = await self._http_account.query_position_info(symbol)
        for position in bybit_positions:
            active_symbols.add(position.symbol)
        return active_symbols

    async def _update_account_state(self) -> None:
        positions = await self._http_account.query_position_info()
        balances = await self._http_account.query_wallet_balance()
        print('tu smo')

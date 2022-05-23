import asyncio
from decimal import Decimal
from typing import Optional

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.msgbus.bus import MessageBus


class SandboxExecClientConfig(LiveExecClientConfig):
    """
    Configuration for ``SandboxExecClient`` instances.

    Parameters
    ----------
    venue : str
        The venue to generate a sandbox execution client for
    currency: str
        The currency for this venue
    balance : int
        The starting balance for this venue
    """

    venue: str
    currency: str
    balance: int


class SandboxExecutionClient(LiveExecutionClient):
    """
    Provides a sandboxed execution client for testing against.

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
    logger : Logger
        The logger for the client.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        account_id: AccountId,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InstrumentProvider,
        venue: str,
        currency: str,
        balance: int,
        oms_type: OMSType = OMSType.NETTING,
        account_type: AccountType = AccountType.CASH,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(venue),
            venue=Venue(venue),
            oms_type=oms_type,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )
        self.exchange: Optional[SimulatedExchange] = None
        self.currency = Currency.from_str(currency)
        money = Money(value=balance, currency=self.currency)
        self.balance = AccountBalance(total=money, locked=Money(0, money.currency), free=money)
        self._logger = logger
        self._test_clock = TestClock()
        self._venue_order_id = 0
        self._set_account_id(account_id)

    def connect(self):
        """
        Connect the client to Sandbox.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._set_connected(True)
        self._log.info("Connected.")
        # self.generate_account_state(
        #     balances=[self.balance],
        #     margins=[],
        #     reported=True,
        #     ts_event=self._clock.timestamp_ns(),
        # )
        await asyncio.sleep(0)
        self.exchange = self.create_exchange()

    def create_exchange(self):
        exchange = SimulatedExchange(
            venue=self.venue,
            oms_type=self.oms_type,
            account_type=self.account_type,
            base_currency=self.currency,
            starting_balances=[self.balance.free],
            default_leverage=Decimal(10),
            leverages={},
            is_frozen_account=True,
            instruments=self._cache.instruments(self.venue),
            modules=[],
            cache=self._cache,
            fill_model=FillModel(),
            latency_model=LatencyModel(0),
            clock=self._test_clock,
            logger=self._logger,
        )
        exchange.exec_client = self
        return exchange

    def disconnect(self):
        """
        Disconnect the client from Interactive Brokers.
        """
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._set_connected(False)
        self._log.info("Disconnected.")

    def submit_order(self, command: SubmitOrder) -> None:
        self.generate_order_submitted(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            ts_event=command.ts_init,
        )
        # TODO Check if in cross with data?
        instrument = self._cache.instrument(command.instrument_id)
        self.venue_order_id += 1
        self.generate_order_accepted(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            venue_order_id=VenueOrderId(f"{self.venue_order_id}"),
            ts_event=command.ts_init,
        )
        quote = self._cache.quote_tick(command.instrument_id)
        fill_price = quote.ask if command.order.side == OrderSide.BUY else quote.bid
        self.generate_order_filled(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            venue_order_id=VenueOrderId(str(self.venue_order_id)),
            venue_position_id=None,
            trade_id=TradeId(UUID4().value),
            order_side=command.order.side,
            order_type=command.order.type,
            last_qty=command.order.quantity,
            last_px=fill_price,
            quote_currency=instrument.quote_currency,
            commission=Money(0, instrument.quote_currency),
            liquidity_side=LiquiditySide.NONE,
            ts_event=command.ts_init,
        )

    def modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError

    def cancel_order(self, command: CancelOrder) -> None:
        self.generate_order_pending_cancel(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            ts_event=command.ts_init,
        )

    async def generate_order_status_reports(
        self,
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
    ):
        return []

    async def generate_trade_reports(
        self, instrument_id=None, venue_order_id=None, start=None, end=None
    ):
        return []

    async def generate_position_status_reports(
        self,
        instrument_id=None,
        start=None,
        end=None,
    ):
        return []

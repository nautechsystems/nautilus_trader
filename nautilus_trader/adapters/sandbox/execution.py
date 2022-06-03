from decimal import Decimal

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookData
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


class SandboxExecutionClient(BacktestExecClient):
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
        self.currency = Currency.from_str(currency)
        money = Money(value=balance, currency=self.currency)
        self.balance = AccountBalance(total=money, locked=Money(0, money.currency), free=money)
        self.test_clock = TestClock()
        self._venue = Venue(venue)
        self._msgbus = msgbus
        self.exchange = SimulatedExchange(
            venue=self._venue,
            oms_type=oms_type,
            account_type=self.account_type,
            base_currency=self.currency,
            starting_balances=[self.balance.free],
            default_leverage=Decimal(10),
            leverages={},
            is_frozen_account=True,
            instruments=cache.instruments(self._venue),
            modules=[],
            cache=cache,
            fill_model=FillModel(),
            latency_model=LatencyModel(0),
            clock=self.test_clock,
            logger=logger,
        )
        super().__init__(
            exchange=self.exchange,
            msgbus=msgbus,
            cache=cache,
            clock=self.test_clock,
            logger=logger,
        )

    def connect(self):
        """
        Connect the client to Sandbox.
        """
        self._log.info("Connecting...")
        self._msgbus.subscribe("data.*", handler=self.on_data)
        self._set_connected(True)
        self._log.info("Connected.")
        self.exchange.register_client(self)

    def disconnect(self):
        """
        Disconnect the client from Interactive Brokers.
        """
        self._log.info("Disconnecting...")
        self._set_connected(False)
        self._log.info("Disconnected.")

    def on_data(self, data: Data):
        # Taken from main backtest loop of BacktestEngine
        if isinstance(data, OrderBookData):
            self.exchange.process_order_book(data)
        elif isinstance(data, QuoteTick):
            self.exchange.process_quote_tick(data)
        elif isinstance(data, TradeTick):
            self.exchange.process_trade_tick(data)
        elif isinstance(data, Bar):
            self.exchange.process_bar(data)
        self.exchange.process(data.ts_init)

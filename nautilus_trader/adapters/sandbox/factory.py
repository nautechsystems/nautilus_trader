import asyncio

from nautilus_trader.adapters.sandbox.execution import SandboxExecClientConfig
from nautilus_trader.adapters.sandbox.execution import SandboxExecutionClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus


class SandboxLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a `Sandbox` live execution client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: SandboxExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls=None,
    ) -> SandboxExecutionClient:
        """
        Create a new Sandbox execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict[str, object]
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.
        client_cls : class, optional
            The internal client constructor. This allows external library and
            testing dependency injection.

        Returns
        -------
        SandboxExecutionClient

        """
        instrument_provider = InstrumentProvider(
            venue=Venue(config.venue),
            logger=logger,
        )

        exec_client = SandboxExecutionClient(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=instrument_provider,
            venue=config.venue,
            balance=config.balance,
            currency=config.currency,
        )
        return exec_client

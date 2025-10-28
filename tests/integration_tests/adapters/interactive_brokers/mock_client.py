import asyncio
from collections.abc import Callable

from ibapi.client import EClient

from nautilus_trader.adapters.interactive_brokers.client.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.client.wrapper import InteractiveBrokersEWrapper
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.common.enums import LogColor
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


class MockEClient(EClient):
    """
    MockEClient is a subclass of EClient which is used for simulating Interactive
    Brokers' client operations.

    This class overloads a few methods of the parent class to better accommodate testing
    needs. More methods can be added as and when needed, depending on the testing
    requirements.

    """

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._next_valid_counter = 0

    def _handle_task(self, handler: Callable, **kwargs):
        # Get the running loop from pytest-asyncio (session-scoped)
        loop = asyncio.get_running_loop()
        if loop.is_running():
            loop.create_task(handler(**kwargs))  # noqa: RUF006
        else:
            loop.run_until_complete(handler(**kwargs))

    #########################################################################
    ################## Market Data
    #########################################################################

    #########################################################################
    ################## Options
    #########################################################################

    #########################################################################
    ################## Orders
    #########################################################################

    #########################################################################
    ################## Account and Portfolio
    #########################################################################

    #########################################################################
    ################## Daily PnL
    #########################################################################

    #########################################################################
    ################## Executions
    #########################################################################

    #########################################################################
    ################## Contract Details
    #########################################################################

    def reqContractDetails(self, reqId: int, contract: IBContract):
        instrument_id = ib_contract_to_instrument_id(contract, contract.exchange)

        match instrument_id.value:
            case "AAPL.NASDAQ":
                self._handle_task(
                    self.wrapper._client.process_contract_details,
                    req_id=reqId,
                    contract_details=IBTestContractStubs.aapl_equity_contract_details(),
                )
            case "EUR/USD.IDEALPRO":
                self._handle_task(
                    self.wrapper._client.process_contract_details,
                    req_id=reqId,
                    contract_details=IBTestContractStubs.eurusd_forex_contract_details(),
                )

        self._handle_task(
            self.wrapper._client.process_contract_details_end,
            req_id=reqId,
        )

    #########################################################################
    ################## Market Depth
    #########################################################################

    #########################################################################
    ################## News Bulletins
    #########################################################################

    #########################################################################
    ################## Financial Advisors
    #########################################################################
    def reqManagedAccts(self):
        self._handle_task(
            self.wrapper._client.process_managed_accounts,
            accounts_list="DU1234567,",
        )

    #########################################################################
    ################## Historical Data
    #########################################################################

    #########################################################################
    ################## Market Scanners
    #########################################################################

    #########################################################################
    ################## Real Time Bars
    #########################################################################

    #########################################################################
    ################## Fundamental Data
    #########################################################################

    ########################################################################
    ################## News
    #########################################################################

    #########################################################################
    ################## Display Groups
    #########################################################################


class MockInteractiveBrokersClient(InteractiveBrokersClient):
    """
    MockInteractiveBrokersClient is a subclass of InteractiveBrokersClient used for
    simulating client operations.

    This class initializes the EClient with a mocked version for testing purposes.

    """

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

        self._eclient = MockEClient(
            wrapper=InteractiveBrokersEWrapper(
                nautilus_logger=self._log,
                client=self,
            ),
        )

    async def _start_async(self):
        self._start_tws_incoming_msg_reader()
        self._start_internal_msg_queue_processor()
        self._eclient.startApi()

        self._is_client_ready.set()
        self._log.debug("`_is_client_ready` set by `_start_async`.", LogColor.BLUE)
        self._connection_attempts = 0

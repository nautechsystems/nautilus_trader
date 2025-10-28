# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal
from functools import partial
from typing import TYPE_CHECKING

from ibapi.commission_report import CommissionReport
from ibapi.common import BarData
from ibapi.common import FaDataType
from ibapi.common import HistogramData
from ibapi.common import ListOfContractDescription
from ibapi.common import ListOfDepthExchanges
from ibapi.common import ListOfFamilyCode
from ibapi.common import ListOfHistoricalSessions
from ibapi.common import ListOfHistoricalTick
from ibapi.common import ListOfHistoricalTickBidAsk
from ibapi.common import ListOfHistoricalTickLast
from ibapi.common import ListOfNewsProviders
from ibapi.common import ListOfPriceIncrements
from ibapi.common import OrderId
from ibapi.common import SetOfFloat
from ibapi.common import SetOfString
from ibapi.common import SmartComponentMap
from ibapi.common import TickAttrib
from ibapi.common import TickAttribBidAsk
from ibapi.common import TickAttribLast
from ibapi.common import TickerId
from ibapi.contract import Contract
from ibapi.contract import ContractDetails
from ibapi.contract import DeltaNeutralContract
from ibapi.execution import Execution
from ibapi.order import Order
from ibapi.order_state import OrderState
from ibapi.ticktype import TickType
from ibapi.utils import current_fn_name
from ibapi.wrapper import EWrapper

from nautilus_trader.common.component import Logger


if TYPE_CHECKING:
    from nautilus_trader.adapters.interactive_brokers.client.client import InteractiveBrokersClient


class InteractiveBrokersEWrapper(EWrapper):
    def __init__(
        self,
        nautilus_logger: Logger,
        client: InteractiveBrokersClient,
    ) -> None:
        super().__init__()
        self._log = nautilus_logger
        self._client = client

    def logAnswer(self, fnName, fnParams) -> None:
        """
        Override the logging for EWrapper.logAnswer.
        """
        if "self" in fnParams:
            prms = dict(fnParams)
            del prms["self"]
        else:
            prms = fnParams

        self._log.debug(f"Msg handled: function={fnName} data={prms}")

    def error(
        self,
        reqId: TickerId,
        errorCode: int,
        errorString: str,
        advancedOrderRejectJson="",
    ) -> None:
        """
        Call this event in response to an error in communication or when TWS needs to
        send a message to the client.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_error,
            req_id=reqId,
            error_code=errorCode,
            error_string=errorString,
            advanced_order_reject_json=advancedOrderRejectJson,
        )
        self._client.submit_to_msg_handler_queue(task)

    def winError(self, text: str, lastError: int) -> None:
        self.logAnswer(current_fn_name(), vars())

    def connectAck(self) -> None:
        """
        Invoke this callback to signify the completion of a successful connection.
        """
        self.logAnswer(current_fn_name(), vars())

    def marketDataType(self, reqId: TickerId, marketDataType: int) -> None:
        """
        Receives notification when the market data type changes.

        This method is called when TWS sends a marketDataType(type) callback to the API,
        where type is set to Frozen or RealTime, to announce that market data has been
        switched between frozen and real-time. This notification occurs only when market
        data switches between real-time and frozen. The marketDataType() callback accepts
        a reqId parameter and is sent per every subscription because different contracts
        can generally trade on a different schedule.

        Parameters
        ----------
        reqId : TickerId
            The request's identifier.
        marketDataType : int
            The type of market data being received. Possible values are 1 for real-time streaming, 2 for frozen market data.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_market_data_type,
            req_id=reqId,
            market_data_type=marketDataType,
        )
        self._client.submit_to_msg_handler_queue(task)

    def tickPrice(
        self,
        reqId: TickerId,
        tickType: TickType,
        price: float,
        attrib: TickAttrib,
    ) -> None:
        """
        Market data tick price callback.

        Parameters
        ----------
        reqId : TickerId
            The request's identifier.
        tickType : TickType
            The type of tick being received.
        price : float
            The price of the tick.
        attrib : TickAttrib
            The tick's attributes.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_tick_price,
            req_id=reqId,
            tick_type=tickType,
            price=price,
            attrib=attrib,
        )
        self._client.submit_to_msg_handler_queue(task)

    def tickSize(self, reqId: TickerId, tickType: TickType, size: Decimal) -> None:
        """
        Handle tick size-related market data.

        This method is responsible for handling all size-related ticks from the market data.
        Each tick represents a change in the market size for a specific type of data.

        Parameters
        ----------
        reqId : TickerId
            The request's identifier.
        tickType : TickType
            The type of tick being received.
        size : Decimal
            The size of the tick.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_tick_size,
            req_id=reqId,
            tick_type=tickType,
            size=size,
        )
        self._client.submit_to_msg_handler_queue(task)

    def tickSnapshotEnd(self, reqId: int) -> None:
        """
        When requesting market data snapshots, this market will indicate the snapshot
        reception is finished.
        """
        self.logAnswer(current_fn_name(), vars())

    def tickGeneric(self, reqId: TickerId, tickType: TickType, value: float) -> None:
        self.logAnswer(current_fn_name(), vars())

    def tickString(self, reqId: TickerId, tickType: TickType, value: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def tickEFP(
        self,
        reqId: TickerId,
        tickType: TickType,
        basisPoints: float,
        formattedBasisPoints: str,
        totalDividends: float,
        holdDays: int,
        futureLastTradeDate: str,
        dividendImpact: float,
        dividendsToLastTradeDate: float,
    ) -> None:
        """
        Market data callback for Exchange for Physical.

        Parameters
        ----------
        reqId : TickerId
            The request's identifier.
        tickType : TickType
            The type of tick being received.
        basisPoints : float
            Annualized basis points, representative of the financing rate that can be directly be
            compared to broker rates.
        formattedBasisPoints : str
            Annualized basis points as a formatted string depicting them in percentage form.
        totalDividends : float
            The total dividends.
        holdDays : int
            The number of hold days until the lastTradeDate of the EFP.
        futureLastTradeDate : str
            The expiration date of the single stock future.
        dividendImpact : float
            The dividend impact upon the annualized basis points interest rate.
        dividendsToLastTradeDate : float
            The dividends expected until the expiration of the single stock future.

        """
        self.logAnswer(current_fn_name(), vars())

    def orderStatus(
        self,
        orderId: OrderId,
        status: str,
        filled: Decimal,
        remaining: Decimal,
        avgFillPrice: float,
        permId: int,
        parentId: int,
        lastFillPrice: float,
        clientId: int,
        whyHeld: str,
        mktCapPrice: float,
    ) -> None:
        """
        Call this event whenever the status of an order changes. Also, fire it after
        reconnecting to TWS if the client has any open orders.

        Parameters
        ----------
        orderId: OrderId
            The order ID that was specified previously in the call to placeOrder().
        status: str
            The order status. Possible values include:
            PendingSubmit, PendingCancel, PreSubmitted, Submitted, Cancelled, Filled, Inactive.
        filled: int
            Specifies the number of shares that have been executed.
        remaining: int
            Specifies the number of shares still outstanding.
        avgFillPrice: float
            The average price of the shares that have been executed.
        permId: int
            The TWS id used to identify orders. Remains the same over TWS sessions.
        parentId: int
            The order ID of the parent order, used for bracket and auto trailing stop orders.
        lastFillPrice: float
            The last price of the shares that have been executed.
        clientId: int
            The ID of the client (or TWS) that placed the order.
        whyHeld: str
            This field is used to identify an order held when TWS is trying to locate shares for a short sell.
            The value used to indicate this is 'locate'.
        mktCapPrice: float
            The price at which the market cap price is calculated.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_order_status,
            order_id=orderId,
            status=status,
            filled=filled,
            remaining=remaining,
            avg_fill_price=avgFillPrice,
            perm_id=permId,
            parent_id=parentId,
            last_fill_price=lastFillPrice,
            client_id=clientId,
            why_held=whyHeld,
            mkt_cap_price=mktCapPrice,
        )
        self._client.submit_to_msg_handler_queue(task)

    def openOrder(
        self,
        orderId: OrderId,
        contract: Contract,
        order: Order,
        orderState: OrderState,
    ) -> None:
        """
        Call this function to feed in open orders.

        Parameters
        ----------
        orderId: OrderId
            The order ID assigned by TWS. Use to cancel or update TWS order.
        contract: Contract
            The Contract class attributes describe the contract.
        order: Order
            The Order class gives the details of the open order.
        orderState: OrderState
            The orderState class includes attributes Used for both pre and post trade margin and commission data.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_open_order,
            order_id=orderId,
            contract=contract,
            order=order,
            order_state=orderState,
        )
        self._client.submit_to_msg_handler_queue(task)

    def openOrderEnd(self) -> None:
        """
        Call this at the end of a given request for open orders.
        """
        self.logAnswer(current_fn_name(), vars())
        self._client.submit_to_msg_handler_queue(
            self._client.process_open_order_end,
        )

    def connectionClosed(self) -> None:
        """
        Call this function when TWS closes the socket connection with the ActiveX
        control, or when TWS is shut down.
        """
        self.logAnswer(current_fn_name(), vars())
        self._client.process_connection_closed()

    def updateAccountValue(
        self,
        key: str,
        val: str,
        currency: str,
        accountName: str,
    ) -> None:
        """
        Call this function only when ReqAccountUpdates on EEClientSocket object has been
        called.
        """
        self.logAnswer(current_fn_name(), vars())

    def updatePortfolio(
        self,
        contract: Contract,
        position: Decimal,
        marketPrice: float,
        marketValue: float,
        averageCost: float,
        unrealizedPNL: float,
        realizedPNL: float,
        accountName: str,
    ) -> None:
        """
        Call this function only when reqAccountUpdates on EEClientSocket object has been
        called.
        """
        self.logAnswer(current_fn_name(), vars())

    def updateAccountTime(self, timeStamp: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def accountDownloadEnd(self, accountName: str) -> None:
        """
        Call this after a batch updateAccountValue() and updatePortfolio() is sent.
        """
        self.logAnswer(current_fn_name(), vars())

    def nextValidId(self, orderId: int) -> None:
        """
        Receives next valid order id.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_next_valid_id,
            order_id=orderId,
        )
        self._client.submit_to_msg_handler_queue(task)

    def contractDetails(self, reqId: int, contractDetails: ContractDetails) -> None:
        """
        Receives the full contract's definitions.

        This method will return all
        contracts matching the requested via EEClientSocket::reqContractDetails.
        For example, one can obtain the whole option chain with it.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_contract_details,
            req_id=reqId,
            contract_details=contractDetails,
        )
        self._client.submit_to_msg_handler_queue(task)

    def bondContractDetails(self, reqId: int, contractDetails: ContractDetails) -> None:
        """
        Call this function when the reqContractDetails function has been called for
        bonds.
        """
        self.logAnswer(current_fn_name(), vars())

    def contractDetailsEnd(self, reqId: int) -> None:
        """
        Call this function once all contract details for a given request are received.

        This helps to define the end of an option chain.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_contract_details_end,
            req_id=reqId,
        )
        self._client.submit_to_msg_handler_queue(task)

    def execDetails(self, reqId: int, contract: Contract, execution: Execution) -> None:
        """
        Fire this event when the reqExecutions() function is invoked or when an order is
        filled.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_exec_details,
            req_id=reqId,
            contract=contract,
            execution=execution,
        )
        self._client.submit_to_msg_handler_queue(task)

    def execDetailsEnd(self, reqId: int) -> None:
        """
        Call this function once all executions have been sent to a client in response to
        reqExecutions().
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_exec_details_end,
            req_id=reqId,
        )
        self._client.submit_to_msg_handler_queue(task)

    def updateMktDepth(
        self,
        reqId: TickerId,
        position: int,
        operation: int,
        side: int,
        price: float,
        size: Decimal,
    ) -> None:
        """
        Return the order book.

        Parameters
        ----------
        reqId : TickerId
            The request's identifier.
        position : int
            The order book's row being updated.
        operation : int
            How to refresh the row:
            - 0: insert (insert this new order into the row identified by 'position')
            - 1: update (update the existing order in the row identified by 'position')
            - 2: delete (delete the existing order at the row identified by 'position').
        side : int
            0 for ask, 1 for bid.
        price : float
            The order's price.
        size : Decimal
            The order's size.

        """
        self.logAnswer(current_fn_name(), vars())

    def updateMktDepthL2(
        self,
        reqId: TickerId,
        position: int,
        marketMaker: str,
        operation: int,
        side: int,
        price: float,
        size: Decimal,
        isSmartDepth: bool,
    ) -> None:
        """
        Return the order book.

        Parameters
        ----------
        reqId : TickerId
            The request's identifier.
        position : int
            The order book's row being updated.
        marketMaker : str
            The exchange holding the order if isSmartDepth is True,
            otherwise the MPID of the market maker.
        operation : int
            How to refresh the row:
            - 0: insert (insert this new order into the row identified by 'position')
            - 1: update (update the existing order in the row identified by 'position')
            - 2: delete (delete the existing order at the row identified by 'position')
        side : int
            0 for ask, 1 for bid.
        price : float
            The order's price.
        size : Decimal
            The order's size.
        isSmartDepth : bool
            Is SMART Depth request.

        """
        self.logAnswer(current_fn_name(), vars())

        task = partial(
            self._client.process_update_mkt_depth_l2,
            req_id=reqId,
            position=position,
            market_maker=marketMaker,
            operation=operation,
            side=side,
            price=price,
            size=size,
            is_smart_depth=isSmartDepth,
        )
        self._client.submit_to_msg_handler_queue(task)

    def updateNewsBulletin(
        self,
        msgId: int,
        msgType: int,
        newsMessage: str,
        originExch: str,
    ) -> None:
        """
        Provide IB's bulletins.

        Parameters
        ----------
        msgId: int
            The bulletin's identifier.
        msgType: int
            One of:
            - 1: Regular news bulletin
            - 2: Exchange no longer available for trading
            - 3: Exchange is available for trading
        newsMessage: str
            The message.
        originExch: str
            The exchange where the message comes from.

        """
        self.logAnswer(current_fn_name(), vars())

    def managedAccounts(self, accountsList: str) -> None:
        """
        Receives a comma-separated string with the managed account ids.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_managed_accounts,
            accounts_list=accountsList,
        )
        self._client.submit_to_msg_handler_queue(task)

    def receiveFA(self, faData: FaDataType, cxml: str) -> None:
        """
        Receives the Financial Advisor's configuration available in the TWS.

        Parameters
        ----------
        faData : str
            One of the following:
            - Groups: Offer traders a way to create a group of accounts and apply a single allocation method
            to all accounts in the group.
            - Account Aliases: Let you easily identify the accounts by meaningful names rather than account numbers.
        cxml : str
            The XML-formatted configuration.

        """
        self.logAnswer(current_fn_name(), vars())

    def historicalData(self, reqId: int, bar: BarData) -> None:
        """
        Return the requested historical data bars.

        Parameters
        ----------
        reqId : int
            The request's identifier.
        bar : BarData
            The bar's data.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_historical_data,
            req_id=reqId,
            bar=bar,
        )
        self._client.submit_to_msg_handler_queue(task)

    def historicalDataEnd(self, reqId: int, start: str, end: str) -> None:
        """
        Mark the end of the reception of historical bars.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_historical_data_end,
            req_id=reqId,
            start=start,
            end=end,
        )
        self._client.submit_to_msg_handler_queue(task)

    def scannerParameters(self, xml: str) -> None:
        """
        Provide the XML-formatted parameters available to create a market scanner.

        Parameters
        ----------
        xml : str
            The XML-formatted string with the available parameters.

        """
        self.logAnswer(current_fn_name(), vars())

    def scannerData(
        self,
        reqId: int,
        rank: int,
        contractDetails: ContractDetails,
        distance: str,
        benchmark: str,
        projection: str,
        legsStr: str,
    ) -> None:
        """
        Provide the data resulting from the market scanner request.

        Parameters
        ----------
        reqId : int
            The request's identifier.
        rank : int
            The ranking within the response of this bar.
        contractDetails : ContractDetails
            The data's ContractDetails.
        distance : str
            According to query.
        benchmark : str
            According to query.
        projection : str
            According to query.
        legsStr : str
            Describes the combo legs when the scanner is returning EFP.

        """
        self.logAnswer(current_fn_name(), vars())

    def scannerDataEnd(self, reqId: int) -> None:
        """
        Indicate that scanner data reception has terminated.

        Parameters
        ----------
        reqId : int
            The request's identifier.

        """
        self.logAnswer(current_fn_name(), vars())

    def realtimeBar(
        self,
        reqId: TickerId,
        time: int,
        open_: float,
        high: float,
        low: float,
        close: float,
        volume: Decimal,
        wap: Decimal,
        count: int,
    ) -> None:
        """
        Update real-time 5-second bars.

        Parameters
        ----------
        reqId : int
            The request's identifier.
        time : int
            Start of the bar in Unix (or 'epoch') time.
        open_ : float
            The bar's open value.
        high : float
            The bar's high value.
        low : float
            The bar's low value.
        close : float
            The bar's closing value.
        volume : int
            The bar's traded volume if available.
        wap : float
            The bar's Weighted Average Price.
        count : int
            The number of trades during the bar's timespan (only available for TRADES).

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_realtime_bar,
            req_id=reqId,
            time=time,
            open_=open_,
            high=high,
            low=low,
            close=close,
            volume=volume,
            wap=wap,
            count=count,
        )
        self._client.submit_to_msg_handler_queue(task)

    def currentTime(self, time: int) -> None:
        """
        Obtain the IB server's system time by calling this method as a result of
        invoking `reqCurrentTime`.
        """
        self.logAnswer(current_fn_name(), vars())

    def fundamentalData(self, reqId: TickerId, data: str) -> None:
        """
        Call this function to receive fundamental market data.

        Ensure that the appropriate market data subscription is set up in Account
        Management before attempting to receive this data.

        """
        self.logAnswer(current_fn_name(), vars())

    def deltaNeutralValidation(
        self,
        reqId: int,
        deltaNeutralContract: DeltaNeutralContract,
    ) -> None:
        """
        When accepting a Delta-Neutral RFQ (request for quote), the server sends a
        deltaNeutralValidation() message with the DeltaNeutralContract structure.

        If the delta and price fields are empty in the original request, the
        confirmation will contain the current values from the server. These values are
        locked when the RFQ is processed and remain locked until the RFQ is canceled.

        """
        self.logAnswer(current_fn_name(), vars())

    def commissionReport(self, commissionReport: CommissionReport) -> None:
        """
        Trigger this callback in the following scenarios:

        - Immediately after a trade execution.
        - By calling reqExecutions().

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_commission_report,
            commission_report=commissionReport,
        )
        self._client.submit_to_msg_handler_queue(task)

    def position(
        self,
        account: str,
        contract: Contract,
        position: Decimal,
        avgCost: float,
    ) -> None:
        """
        Return real-time positions for all accounts in response to the reqPositions()
        method.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_position,
            account_id=account,
            contract=contract,
            position=position,
            avg_cost=avgCost,
        )
        self._client.submit_to_msg_handler_queue(task)

    def positionEnd(self) -> None:
        """
        Call this once all position data for a given request has been received, serving
        as an end marker for the position() data.
        """
        self.logAnswer(current_fn_name(), vars())
        self._client.submit_to_msg_handler_queue(
            self._client.process_position_end,
        )

    def accountSummary(
        self,
        reqId: int,
        account: str,
        tag: str,
        value: str,
        currency: str,
    ) -> None:
        """
        Return the data from the TWS Account Window Summary tab in response to
        reqAccountSummary().
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_account_summary,
            req_id=reqId,
            account_id=account,
            tag=tag,
            value=value,
            currency=currency,
        )
        self._client.submit_to_msg_handler_queue(task)

    def accountSummaryEnd(self, reqId: int) -> None:
        """
        Call this method when all account summary data for a given request has been
        received.
        """
        self.logAnswer(current_fn_name(), vars())

    def verifyCompleted(self, isSuccessful: bool, errorText: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def verifyAndAuthMessageAPI(self, apiData: str, xyzChallenge: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def verifyAndAuthCompleted(self, isSuccessful: bool, errorText: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def displayGroupList(self, reqId: int, groups: str) -> None:
        """
        Receive a one-time response callback to queryDisplayGroups().

        Parameters
        ----------
        reqId : int
            The requestId specified in queryDisplayGroups().
        groups : str
            A list of integers representing visible group IDs separated by the '|' character, sorted by most
            used group first. This list remains unchanged during the TWS session (i.e., users cannot add new
            groups; sorting can change).

        """
        self.logAnswer(current_fn_name(), vars())

    def displayGroupUpdated(self, reqId: int, contractInfo: str) -> None:
        """
        Receive a notification from TWS to the API client after subscribing to group
        events via subscribeToGroupEvents(). This notification will be resent if the
        chosen contract in the subscribed display group changes.

        Parameters
        ----------
        reqId : int
            The requestId specified in subscribeToGroupEvents().
        contractInfo : str
            The encoded value uniquely representing the contract in IB. Possible values include:
            - 'none': Empty selection.
            - 'contractID@exchange': For any non-combination contract.
                                     Examples: '8314@SMART' for IBM SMART; '8314@ARCA' for IBM @ARCA.
            - 'combo': If any combo is selected.

        """
        self.logAnswer(current_fn_name(), vars())

    def positionMulti(
        self,
        reqId: int,
        account: str,
        modelCode: str,
        contract: Contract,
        pos: Decimal,
        avgCost: float,
    ) -> None:
        """
        Retrieve the position for a specific account or model, mirroring the position()
        function.
        """
        self.logAnswer(current_fn_name(), vars())

    def positionMultiEnd(self, reqId: int) -> None:
        """
        Terminate the position for a specific account or model, akin to the
        positionEnd() function.
        """
        self.logAnswer(current_fn_name(), vars())

    def accountUpdateMulti(
        self,
        reqId: int,
        account: str,
        modelCode: str,
        key: str,
        value: str,
        currency: str,
    ) -> None:
        """
        Update the value for a specific account or model, similar to the
        updateAccountValue() function.
        """
        self.logAnswer(current_fn_name(), vars())

    def accountUpdateMultiEnd(self, reqId: int) -> None:
        """
        Download data for a specific account or model, resembling accountDownloadEnd()
        functionality.
        """
        self.logAnswer(current_fn_name(), vars())

    def tickOptionComputation(
        self,
        reqId: TickerId,
        tickType: TickType,
        tickAttrib: int,
        impliedVol: float,
        delta: float,
        optPrice: float,
        pvDividend: float,
        gamma: float,
        vega: float,
        theta: float,
        undPrice: float,
    ) -> None:
        """
        Invoke this function in response to market movements in an option or its
        underlier.

        Receive TWS's option model volatilities, prices, and deltas, as well as the
        present value of dividends expected on the option's underlier.

        """
        self.logAnswer(current_fn_name(), vars())

    def securityDefinitionOptionParameter(
        self,
        reqId: int,
        exchange: str,
        underlyingConId: int,
        tradingClass: str,
        multiplier: str,
        expirations: SetOfString,
        strikes: SetOfFloat,
    ) -> None:
        """
        Return the option chain for an underlying on a specified exchange.

        This is triggered by a call to `reqSecDefOptParams`. If multiple exchanges are specified in
        `reqSecDefOptParams`, there will be multiple callbacks to `securityDefinitionOptionParameter`.

        Parameters
        ----------
        reqId : int
            ID of the request that initiated the callback.
        exchange : str
            The exchange for which the option chain is requested.
        underlyingConId : int
            The conID of the underlying security.
        tradingClass : str
            The option trading class.
        multiplier : str
            The option multiplier.
        expirations : list[str]
            A list of expiry dates for the options of this underlying on this exchange.
        strikes : list[float]
            A list of possible strikes for options of this underlying on this exchange.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_security_definition_option_parameter,
            req_id=reqId,
            exchange=exchange,
            underlying_con_id=underlyingConId,
            trading_class=tradingClass,
            multiplier=multiplier,
            expirations=expirations,
            strikes=strikes,
        )
        self._client.submit_to_msg_handler_queue(task)

    def securityDefinitionOptionParameterEnd(self, reqId: int) -> None:
        """
        Invoke this after all callbacks to securityDefinitionOptionParameter have been
        completed.

        Parameters
        ----------
        reqId : int
            The ID used in the initial call to `securityDefinitionOptionParameter`.

        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_security_definition_option_parameter_end,
            req_id=reqId,
        )
        self._client.submit_to_msg_handler_queue(task)

    def softDollarTiers(self, reqId: int, tiers: list) -> None:
        """
        Invoke this upon receiving Soft Dollar Tier configuration information.

        Call this method when Soft Dollar Tier configuration details are received.

        Parameters
        ----------
        reqId : int
            The request ID used in the call to `EEClient::reqSoftDollarTiers`.
        tiers : list[SoftDollarTier]
            A list containing all Soft Dollar Tier information.

        """
        self.logAnswer(current_fn_name(), vars())

    def familyCodes(self, familyCodes: ListOfFamilyCode) -> None:
        """
        Return an array of family codes.
        """
        self.logAnswer(current_fn_name(), vars())

    def symbolSamples(
        self,
        reqId: int,
        contractDescriptions: ListOfContractDescription,
    ) -> None:
        """
        Return an array of sample contract descriptions.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_symbol_samples,
            req_id=reqId,
            contract_descriptions=contractDescriptions,
        )
        self._client.submit_to_msg_handler_queue(task)

    def mktDepthExchanges(self, depthMktDataDescriptions: ListOfDepthExchanges) -> None:
        """
        Return an array of exchanges that provide depth data to UpdateMktDepthL2.
        """
        self.logAnswer(current_fn_name(), vars())

    def tickNews(
        self,
        tickerId: int,
        timeStamp: int,
        providerCode: str,
        articleId: str,
        headline: str,
        extraData: str,
    ) -> None:
        """
        Return news headlines.
        """
        self.logAnswer(current_fn_name(), vars())

    def smartComponents(self, reqId: int, smartComponentMap: SmartComponentMap) -> None:
        """
        Return exchange component mapping.
        """
        self.logAnswer(current_fn_name(), vars())

    def tickReqParams(
        self,
        tickerId: int,
        minTick: float,
        bboExchange: str,
        snapshotPermissions: int,
    ) -> None:
        """
        Return the exchange map for a specific contract.
        """
        self.logAnswer(current_fn_name(), vars())

    def newsProviders(self, newsProviders: ListOfNewsProviders) -> None:
        """
        Return available and subscribed API news providers.
        """
        self.logAnswer(current_fn_name(), vars())

    def newsArticle(self, requestId: int, articleType: int, articleText: str) -> None:
        """
        Return the body of a news article.
        """
        self.logAnswer(current_fn_name(), vars())

    def historicalNews(
        self,
        requestId: int,
        time: str,
        providerCode: str,
        articleId: str,
        headline: str,
    ) -> None:
        """
        Return historical news headlines.
        """
        self.logAnswer(current_fn_name(), vars())

    def historicalNewsEnd(self, requestId: int, hasMore: bool) -> None:
        """
        Signals end of historical news.
        """
        self.logAnswer(current_fn_name(), vars())

    def headTimestamp(self, reqId: int, headTimestamp: str) -> None:
        """
        Return the earliest available data for a specific type of data for a given
        contract.
        """
        self.logAnswer(current_fn_name(), vars())

    def histogramData(self, reqId: int, items: HistogramData) -> None:
        """
        Return histogram data for a contract.
        """
        self.logAnswer(current_fn_name(), vars())

    def historicalDataUpdate(self, reqId: int, bar: BarData) -> None:
        """
        Return updates in real time when keepUpToDate is set to True.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_historical_data_update,
            req_id=reqId,
            bar=bar,
        )
        self._client.submit_to_msg_handler_queue(task)

    def rerouteMktDataReq(self, reqId: int, conId: int, exchange: str) -> None:
        """
        Return rerouted CFD contract information for a market data request.
        """
        self.logAnswer(current_fn_name(), vars())

    def rerouteMktDepthReq(self, reqId: int, conId: int, exchange: str) -> None:
        """
        Return rerouted CFD contract information for a market depth request.
        """
        self.logAnswer(current_fn_name(), vars())

    def marketRule(self, marketRuleId: int, priceIncrements: ListOfPriceIncrements) -> None:
        """
        Return the minimum price increment structure for a specific market rule ID.
        """
        self.logAnswer(current_fn_name(), vars())

    def pnl(self, reqId: int, dailyPnL: float, unrealizedPnL: float, realizedPnL: float) -> None:
        """
        Return the daily Profit and Loss (PnL) for the account.
        """
        self.logAnswer(current_fn_name(), vars())

    def pnlSingle(
        self,
        reqId: int,
        pos: Decimal,
        dailyPnL: float,
        unrealizedPnL: float,
        realizedPnL: float,
        value: float,
    ) -> None:
        """
        Return the daily Profit and Loss (PnL) for a single position in the account.
        """
        self.logAnswer(current_fn_name(), vars())

    def historicalTicks(self, reqId: int, ticks: ListOfHistoricalTick, done: bool) -> None:
        """
        Return historical tick data when whatToShow is set to MIDPOINT.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_historical_ticks,
            req_id=reqId,
            ticks=ticks,
            done=done,
        )
        self._client.submit_to_msg_handler_queue(task)

    def historicalTicksBidAsk(
        self,
        reqId: int,
        ticks: ListOfHistoricalTickBidAsk,
        done: bool,
    ) -> None:
        """
        Return historical tick data when whatToShow is set to BID_ASK.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_historical_ticks_bid_ask,
            req_id=reqId,
            ticks=ticks,
            done=done,
        )
        self._client.submit_to_msg_handler_queue(task)

    def historicalTicksLast(self, reqId: int, ticks: ListOfHistoricalTickLast, done: bool) -> None:
        """
        Return historical tick data when whatToShow is set to TRADES.
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_historical_ticks_last,
            req_id=reqId,
            ticks=ticks,
            done=done,
        )
        self._client.submit_to_msg_handler_queue(task)

    def tickByTickAllLast(
        self,
        reqId: int,
        tickType: int,
        time: int,
        price: float,
        size: Decimal,
        tickAttribLast: TickAttribLast,
        exchange: str,
        specialConditions: str,
    ) -> None:
        """
        Return tick-by-tick data for tickType set to "Last" or "AllLast".
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_tick_by_tick_all_last,
            req_id=reqId,
            tick_type=tickType,
            time=time,
            price=price,
            size=size,
            tick_attrib_last=tickAttribLast,
            exchange=exchange,
            special_conditions=specialConditions,
        )
        self._client.submit_to_msg_handler_queue(task)

    def tickByTickBidAsk(
        self,
        reqId: int,
        time: int,
        bidPrice: float,
        askPrice: float,
        bidSize: Decimal,
        askSize: Decimal,
        tickAttribBidAsk: TickAttribBidAsk,
    ) -> None:
        """
        Return tick-by-tick data for tickType set to "BidAsk".
        """
        self.logAnswer(current_fn_name(), vars())
        task = partial(
            self._client.process_tick_by_tick_bid_ask,
            req_id=reqId,
            time=time,
            bid_price=bidPrice,
            ask_price=askPrice,
            bid_size=bidSize,
            ask_size=askSize,
            tick_attrib_bid_ask=tickAttribBidAsk,
        )
        self._client.submit_to_msg_handler_queue(task)

    def tickByTickMidPoint(self, reqId: int, time: int, midPoint: float) -> None:
        """
        Return tick-by-tick data for tickType set to "MidPoint".
        """
        self.logAnswer(current_fn_name(), vars())

    def orderBound(self, reqId: int, apiClientId: int, apiOrderId: int) -> None:
        """
        Return the orderBound notification.
        """
        self.logAnswer(current_fn_name(), vars())

    def completedOrder(self, contract: Contract, order: Order, orderState: OrderState) -> None:
        """
        Feed in completed orders.

        Call this function to provide information on completed orders.

        Parameters
        ----------
        contract : Contract
            Describes the contract with attributes of the Contract class.
        order : Order
            Details of the completed order, as defined by the Order class.
        orderState : OrderState
            Includes status details of the completed order, as specified in the OrderState class.

        """
        self.logAnswer(current_fn_name(), vars())

    def completedOrdersEnd(self) -> None:
        """
        Invoke this upon completing a request for completed orders.
        """
        self.logAnswer(current_fn_name(), vars())

    def replaceFAEnd(self, reqId: int, text: str) -> None:
        """
        Invoke this at the completion of a Financial Advisor (FA) replacement operation.
        """
        self.logAnswer(current_fn_name(), vars())

    def wshMetaData(self, reqId: int, dataJson: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def wshEventData(self, reqId: int, dataJson: str) -> None:
        self.logAnswer(current_fn_name(), vars())

    def historicalSchedule(
        self,
        reqId: int,
        startDateTime: str,
        endDateTime: str,
        timeZone: str,
        sessions: ListOfHistoricalSessions,
    ) -> None:
        """
        Return historical schedule for historical data request with whatToShow=SCHEDULE.
        """
        self.logAnswer(current_fn_name(), vars())

    def userInfo(self, reqId: int, whiteBrandingId: str) -> None:
        """
        Return user info.
        """
        self.logAnswer(current_fn_name(), vars())

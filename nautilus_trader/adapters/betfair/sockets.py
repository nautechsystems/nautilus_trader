from betfairlightweight import APIClient
from betfairlightweight.filters import streaming_market_data_filter
from betfairlightweight.filters import streaming_market_filter
from betfairlightweight.filters import streaming_order_filter

from nautilus_trader.data.socket import SocketClient


HOST = "stream-api.betfair.com"
PORT = 443
CRLF = b"\r\n"
ENCODING = "utf-8"
_UNIQUE_ID = 0


class BetfairStreamClient(SocketClient):
    def __init__(
        self,
        client: APIClient,
        message_handler,
        loop=None,
        host=None,
        port=None,
        crlf=None,
        encoding=None,
    ):
        super().__init__(
            loop=loop,
            host=host or HOST,
            port=port or PORT,
            message_handler=message_handler,
            crlf=crlf or CRLF,
            encoding=encoding or ENCODING,
        )
        self.client = client
        self.unique_id = self.new_unique_id()

    async def connect(self):
        assert (
            self.client.session_token
        ), f"Must login to APIClient before calling connect on {self.__class__}"
        return await super().connect()

    def new_unique_id(self) -> int:
        global _UNIQUE_ID
        _UNIQUE_ID += 1
        return _UNIQUE_ID

    def auth_message(self):
        return {
            "op": "authentication",
            "id": self.unique_id,
            "appKey": self.client.app_key,
            "session": self.client.session_token,
        }


class BetfairOrderStreamClient(BetfairStreamClient):
    def __init__(
        self,
        client: APIClient,
        message_handler,
        partition_matched_by_strategy_ref=True,
        include_overall_position=None,
        customer_strategy_refs=None,
        **kwargs,
    ):
        super().__init__(client=client, message_handler=message_handler, **kwargs)
        self.order_filter = streaming_order_filter(
            include_overall_position=include_overall_position,
            customer_strategy_refs=customer_strategy_refs,
            partition_matched_by_strategy_ref=partition_matched_by_strategy_ref,
        )

    async def post_connection(self):
        subscribe_msg = {
            "op": "orderSubscription",
            "id": self.unique_id,
            "orderFilter": self.order_filter,
            "initialClk": None,
            "clk": None,
        }
        await self.send(raw=self.auth_message())
        await self.send(raw=subscribe_msg)


class BetfairMarketStreamClient(BetfairStreamClient):
    def __init__(self, client: APIClient, message_handler, **kwargs):
        self.subscription_message = None
        super().__init__(client, message_handler, **kwargs)

    def set_market_subscriptions(
        self,
        market_ids: list = None,
        betting_types: list = None,
        event_type_ids: list = None,
        event_ids: list = None,
        turn_in_play_enabled: bool = None,
        market_types: list = None,
        venues: list = None,
        country_codes: list = None,
        race_types: list = None,
        subscribe_book_updates=True,
        subscribe_trade_updates=True,
    ):
        """
        See `betfairlightweight.filters.streaming_market_filter` for full docstring

        :param market_ids:
        :param betting_types:
        :param event_type_ids:
        :param event_ids:
        :param turn_in_play_enabled:
        :param market_types:
        :param venues:
        :param country_codes:
        :param race_types:
        :param subscribe_book_updates: Subscribe to market orderbook events
        :param subscribe_trade_updates: Subscribe to market trade events
        :return:
        """
        filters = (
            market_ids,
            betting_types,
            event_type_ids,
            event_ids,
            turn_in_play_enabled,
            market_types,
            venues,
            country_codes,
            race_types,
        )
        assert any(filters), "Must pass at least one filter"
        assert any(
            (subscribe_book_updates, subscribe_trade_updates)
        ), "Must subscribe to either book updates or trades"
        if market_ids is not None:
            # TODO - Log a warning about inefficiencies of specific market ids - Won't receive any updates for new
            #  markets that fit criteria like when using event type / market type etc
            # logging.warning()
            pass
        market_filter = streaming_market_filter(
            market_ids=market_ids,
            betting_types=betting_types,
            event_type_ids=event_type_ids,
            event_ids=event_ids,
            turn_in_play_enabled=turn_in_play_enabled,
            market_types=market_types,
            venues=venues,
            country_codes=country_codes,
            race_types=race_types,
        )
        data_fields = []
        if subscribe_book_updates:
            data_fields.append("EX_ALL_OFFERS")
        if subscribe_trade_updates:
            data_fields.append("EX_TRADED")
        market_data_filter = streaming_market_data_filter(
            fields=data_fields,
        )

        self._set_subscription_message(
            market_filter=market_filter, market_data_filter=market_data_filter
        )

    # TODO - Add support for initial_clk/clk reconnection
    def _set_subscription_message(
        self,
        market_filter: dict,
        market_data_filter: dict,
        initial_clk: str = None,
        clk: str = None,
        conflate_ms: int = None,
        heartbeat_ms: int = None,
        segmentation_enabled: bool = True,
    ):
        """
        Market subscription request.
        :param dict market_filter: Market filter
        :param dict market_data_filter: Market data filter
        :param str initial_clk: Sequence token for reconnect
        :param str clk: Sequence token for reconnect
        :param int conflate_ms: conflation rate (bounds are 0 to 120000)
        :param int heartbeat_ms: heartbeat rate (500 to 5000)
        :param bool segmentation_enabled: allow the server to send large sets of data
        in segments, instead of a single block
        """
        message = {
            "op": "marketSubscription",
            "id": self.unique_id,
            "marketFilter": market_filter,
            "marketDataFilter": market_data_filter,
            "initialClk": initial_clk,
            "clk": clk,
            "conflateMs": conflate_ms,
            "heartbeatMs": heartbeat_ms,
            "segmentationEnabled": segmentation_enabled,
        }

        self.subscription_message = message

    async def post_connection(self):
        err = "Must call `set_subscription_message` before attempting connection"
        assert self.subscription_message is not None, err
        await self.send(raw=self.auth_message)
        await self.send(raw=self.subscription_message)

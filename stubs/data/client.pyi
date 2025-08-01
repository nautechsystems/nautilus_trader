from typing import Any

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import Component  # Base class for DataClient and MarketDataClient
from nautilus_trader.core.nautilus_pyo3 import Data
from nautilus_trader.core.nautilus_pyo3 import DataType
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MessageBus
from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import Venue
from stubs.cache.cache import Cache
from stubs.common.component import Clock
from stubs.data.messages import RequestBars, RequestData, RequestInstrument, RequestInstruments, RequestOrderBookSnapshot, RequestQuoteTicks, RequestTradeTicks, SubscribeBars, SubscribeData, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeOrderBook, SubscribeQuoteTicks, SubscribeTradeTicks, UnsubscribeBars, UnsubscribeData, UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeOrderBook, UnsubscribeQuoteTicks, UnsubscribeTradeTicks

class DataClient(Component):
    """
    The base class for all data clients.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    clock : Clock
        The clock for the client.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    config : NautilusConfig, optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    venue: Venue | None
    is_connected: bool

    def __init__(
        self,
        client_id: ClientId,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        venue: Venue | None = None,
        config: NautilusConfig | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def _set_connected(self, value: bool = True) -> None:
        """
        Setter for Python implementations to change the readonly property.

        Parameters
        ----------
        value : bool
            The value to set for is_connected.

        """
        ...
    def subscribed_custom_data(self) -> list[DataType]:
        """
        Return the custom data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        ...
    def subscribe(self, command: SubscribeData) -> None:
        """
        Subscribe to data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        ...
    def unsubscribe(self, command: UnsubscribeData) -> None:
        """
        Unsubscribe from data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.

        """
        ...
    def _add_subscription(self, data_type: DataType) -> None: ...
    def _remove_subscription(self, data_type: DataType) -> None: ...
    def request(self, request: RequestData) -> None:
        """
        Request data for the given data type.

        Parameters
        ----------
        request : RequestData
            The message for the data request.

        """
        ...
    def _handle_data_py(self, data: Data) -> None: ...
    def _handle_data_response_py(self, data_type: DataType, data: Any, correlation_id: UUID4, params: dict[str, object]) -> None: ...
    def _handle_data(self, data: Data) -> None: ...
    def _handle_data_response(self, data_type: DataType, data: Any, correlation_id: UUID4, params: dict[str, object]) -> None: ...


class MarketDataClient(DataClient):
    """
    The base class for all market data clients.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    config : NautilusConfig, optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        client_id: ClientId,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        venue: Venue | None = None,
        config: NautilusConfig | None = None,
    ) -> None: ...
    def subscribed_custom_data(self) -> list[DataType]:
        """
        Return the custom data types subscribed to.

        Returns
        -------
        list[DataType]

        """
        ...
    def subscribed_instruments(self) -> list[InstrumentId]:
        """
        Return the instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_order_book_deltas(self) -> list[InstrumentId]:
        """
        Return the order book delta instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_order_book_snapshots(self) -> list[InstrumentId]:
        """
        Return the order book snapshot instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_quote_ticks(self) -> list[InstrumentId]:
        """
        Return the quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_trade_ticks(self) -> list[InstrumentId]:
        """
        Return the trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_mark_prices(self) -> list[InstrumentId]:
        """
        Return the mark price update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_index_prices(self) -> list[InstrumentId]:
        """
        Return the index price update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_bars(self) -> list[BarType]:
        """
        Return the bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        ...
    def subscribed_instrument_status(self) -> list[InstrumentId]:
        """
        Return the status update instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribed_instrument_close(self) -> list[InstrumentId]:
        """
        Return the instrument closes subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def subscribe(self, command: SubscribeData) -> None:
        """
        Subscribe to data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_instruments(self, command: SubscribeInstruments) -> None:
        """
        Subscribe to all `Instrument` data.

        Parameters
        ----------
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_instrument(self, command: SubscribeInstrument) -> None:
        """
        Subscribe to the `Instrument` with the given instrument ID.

        Parameters
        ----------
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to `OrderBookDeltas` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional, default None
            The maximum depth for the subscription.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to `OrderBook` snapshots data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book level.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        """
        Subscribe to `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        """
        Subscribe to `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        """
        Subscribe to `MarkPriceUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        """
        Subscribe to `IndexPriceUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        """
        Subscribe to `InstrumentStatus` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        """
        Subscribe to `InstrumentClose` updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def subscribe_bars(self, command: SubscribeBars) -> None:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe(self, command: UnsubscribeData) -> None:
        """
        Unsubscribe from data for the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type for the subscription.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        """
        Unsubscribe from all `Instrument` data.

        Parameters
        ----------
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        """
        Unsubscribe from `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        """
        Unsubscribe from `OrderBookDeltas` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        """
        Unsubscribe from `OrderBook` snapshots data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        """
        Unsubscribe from `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        """
        Unsubscribe from `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        """
        Unsubscribe from `MarkPriceUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        """
        Unsubscribe from `IndexPriceUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        """
        Unsubscribe from `InstrumentStatus` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument status updates to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        """
        Unsubscribe from `InstrumentClose` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.
        params : dict[str, Any], optional
            Additional params for the subscription.

        """
        ...
    def _add_subscription(self, data_type: DataType) -> None: ...
    def _add_subscription_instrument(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_order_book_deltas(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_order_book_snapshots(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_quote_ticks(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_trade_ticks(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_mark_prices(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_index_prices(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_bars(self, bar_type: BarType) -> None: ...
    def _add_subscription_instrument_status(self, instrument_id: InstrumentId) -> None: ...
    def _add_subscription_instrument_close(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription(self, data_type: DataType) -> None: ...
    def _remove_subscription_instrument(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_order_book_deltas(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_order_book_snapshots(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_quote_ticks(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_trade_ticks(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_mark_prices(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_index_prices(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_bars(self, bar_type: BarType) -> None: ...
    def _remove_subscription_instrument_status(self, instrument_id: InstrumentId) -> None: ...
    def _remove_subscription_instrument_close(self, instrument_id: InstrumentId) -> None: ...
    def request_instrument(self, request: RequestInstrument) -> None:
        """
        Request `Instrument` data for the given instrument ID.

        Parameters
        ----------
        request : RequestInstrument
            The message for the data request.

        """
        ...
    def request_instruments(self, request: RequestInstruments) -> None:
        """
        Request all `Instrument` data for the given venue.

        Parameters
        ----------
        request : RequestInstruments
            The message for the data request.

        """
        ...
    def request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        """
        Request order book snapshot data.

        Parameters
        ----------
        request : RequestOrderBookSnapshot
            The message for the data request.

        """
        ...
    def request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        """
        Request historical `QuoteTick` data.

        Parameters
        ----------
        request : RequestQuoteTicks
            The message for the data request.

        """
        ...
    def request_trade_ticks(self, request: RequestTradeTicks) -> None:
        """
        Request historical `TradeTick` data.

        Parameters
        ----------
        request : RequestTradeTicks
            The message for the data request.

        """
        ...
    def request_bars(self, request: RequestBars) -> None:
        """
        Request historical `Bar` data. To load historical data from a catalog, you can pass a list[DataCatalogConfig] to the TradingNodeConfig or the BacktestEngineConfig.

        Parameters
        ----------
        request : RequestBars
            The message for the data request.

        """
        ...
    def _handle_data_py(self, data: Data) -> None: ...
    def _handle_instrument_py(self, instrument: Instrument, correlation_id: UUID4, params: dict[str, object] | None = None) -> None: ...
    def _handle_instruments_py(self, venue: Venue, instruments: list, correlation_id: UUID4, params: dict[str, object] | None = None) -> None: ...
    def _handle_quote_ticks_py(self, instrument_id: InstrumentId, ticks: list, correlation_id: UUID4, params: dict[str, object] | None = None) -> None: ...
    def _handle_trade_ticks_py(self, instrument_id: InstrumentId, ticks: list, correlation_id: UUID4, params: dict[str, object] | None = None) -> None: ...
    def _handle_bars_py(self, bar_type: BarType, bars: list, partial: Bar, correlation_id: UUID4, params: dict[str, object] | None = None) -> None: ...
    def _handle_data_response_py(self, data_type: DataType, data: Any, correlation_id: UUID4, params: dict[str, object] | None = None) -> None: ...
    def _handle_data(self, data: Data) -> None: ...
    def _handle_instrument(self, instrument: Instrument, correlation_id: UUID4, params: dict[str, object]) -> None: ...
    def _handle_instruments(self, venue: Venue, instruments: list, correlation_id: UUID4, params: dict[str, object]) -> None: ...
    def _handle_quote_ticks(self, instrument_id: InstrumentId, ticks: list, correlation_id: UUID4, params: dict[str, object]) -> None: ...
    def _handle_trade_ticks(self, instrument_id: InstrumentId, ticks: list, correlation_id: UUID4, params: dict[str, object]) -> None: ...
    def _handle_bars(self, bar_type: BarType, bars: list, partial: Bar, correlation_id: UUID4, params: dict[str, object]) -> None: ...
    def _handle_data_response(self, data_type: DataType, data: Any, correlation_id: UUID4, params: dict[str, object]) -> None: ...

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

from cpython.datetime cimport datetime

from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Request
from nautilus_trader.core.message cimport Response
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue


cdef class DataCommand(Command):
    cdef readonly ClientId client_id
    """The data client ID for the command.\n\n:returns: `ClientId` or ``None``"""
    cdef readonly Venue venue
    """The venue for the command.\n\n:returns: `Venue` or ``None``"""
    cdef readonly DataType data_type
    """The command data type.\n\n:returns: `type`"""
    cdef readonly dict[str, object] params
    """Additional specific parameters for the command.\n\n:returns: `dict[str, object]` or ``None``"""


cdef class SubscribeData(DataCommand):
    pass


cdef class SubscribeInstruments(SubscribeData):
    pass


cdef class SubscribeInstrument(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class SubscribeOrderBook(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""
    cdef readonly BookType book_type
    """The order book type."""
    cdef readonly int depth
    """The maximum depth for the subscription."""
    cdef readonly bint managed
    """If an order book should be managed by the data engine based on the subscribed feed."""
    cdef readonly int interval_ms
    """The order book snapshot interval in milliseconds (must be positive)."""
    cdef readonly bint only_deltas
    """If the subscription is for OrderBookDeltas or OrderBook snapshots."""


cdef class SubscribeQuoteTicks(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class SubscribeTradeTicks(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class SubscribeMarkPrices(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class SubscribeIndexPrices(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class SubscribeBars(SubscribeData):
    cdef readonly BarType bar_type
    """The bar type for the subscription."""
    cdef readonly bint await_partial
    """If the bar aggregator should await the arrival of a historical partial bar prior to actively aggregating new bars."""


cdef class SubscribeInstrumentStatus(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class SubscribeInstrumentClose(SubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeData(DataCommand):
    pass


cdef class UnsubscribeInstruments(UnsubscribeData):
    pass


cdef class UnsubscribeInstrument(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeOrderBook(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""
    cdef readonly bint only_deltas
    """If the subscription is for OrderBookDeltas or OrderBook snapshots."""


cdef class UnsubscribeQuoteTicks(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeTradeTicks(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeMarkPrices(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeIndexPrices(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeBars(UnsubscribeData):
    cdef readonly BarType bar_type
    """The bar type for the subscription.\n\n:returns: `BarType`"""


cdef class UnsubscribeInstrumentStatus(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class UnsubscribeInstrumentClose(UnsubscribeData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the subscription.\n\n:returns: `InstrumentId` or ``None``"""


cdef class RequestData(Request):
    cdef readonly DataType data_type
    """The request data type.\n\n:returns: `type`"""
    cdef readonly datetime start
    """The start datetime (UTC) of request time range (inclusive)."""
    cdef readonly datetime end
    """The end datetime (UTC) of request time range."""
    cdef readonly int limit
    """The limit on the amount of data to return for the request."""
    cdef readonly ClientId client_id
    """The data client ID for the request.\n\n:returns: `ClientId` or ``None``"""
    cdef readonly Venue venue
    """The venue for the request.\n\n:returns: `Venue` or ``None``"""
    cdef readonly dict[str, object] params
    """Additional specific parameters for the command.\n\n:returns: `dict[str, object]` or ``None``"""


cdef class RequestInstrument(RequestData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the request.\n\n:returns: `InstrumentId`"""


cdef class RequestInstruments(RequestData):
    pass


cdef class RequestOrderBookSnapshot(RequestData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the request.\n\n:returns: `InstrumentId`"""


cdef class RequestQuoteTicks(RequestData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the request.\n\n:returns: `InstrumentId`"""


cdef class RequestTradeTicks(RequestData):
    cdef readonly InstrumentId instrument_id
    """The instrument ID for the request.\n\n:returns: `InstrumentId`"""


cdef class RequestBars(RequestData):
    cdef readonly BarType bar_type
    """The bar type for the request.\n\n:returns: `BarType`"""


cdef class DataResponse(Response):
    cdef readonly ClientId client_id
    """The data client ID for the response.\n\n:returns: `ClientId` or ``None``"""
    cdef readonly Venue venue
    """The venue for the response.\n\n:returns: `Venue` or ``None``"""
    cdef readonly DataType data_type
    """The response data type.\n\n:returns: `type`"""
    cdef readonly object data
    """The response data.\n\n:returns: `object`"""
    cdef readonly dict[str, object] params
    """Additional specific parameters for the response.\n\n:returns: `dict[str, object]` or ``None``"""


cdef inline str form_params_str(dict[str, object] params):
    return "" if not params else f", params={params}"

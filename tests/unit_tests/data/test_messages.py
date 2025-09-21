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

import pandas as pd
import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestOrderBookDepth
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


BINANCE = Venue("BINANCE")
IDEALPRO = Venue("IDEALPRO")


class TestDataMessage:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_data_messages_when_client_id_and_venue_none_raise_value_error(self):
        # Arrange, Act , Assert
        with pytest.raises(ValueError) as e:
            SubscribeData(
                instrument_id=None,
                client_id=None,
                venue=None,
                data_type=DataType(Data, {"type": "newswire"}),
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
        assert issubclass(e.type, ValueError)
        assert e.match("Both `client_id` and `venue` were None")

        with pytest.raises(ValueError) as e:
            UnsubscribeData(
                instrument_id=None,
                client_id=None,
                venue=None,
                data_type=DataType(Data, {"type": "newswire"}),
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
        assert issubclass(e.type, ValueError)
        assert e.match("Both `client_id` and `venue` were None")

        with pytest.raises(ValueError) as e:
            handler = []
            RequestData(
                data_type=DataType(QuoteTick),
                instrument_id=None,
                start=None,
                end=None,
                limit=0,
                client_id=None,
                venue=None,
                callback=handler.append,
                request_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                params=None,
            )
        assert issubclass(e.type, ValueError)
        assert e.match("Both `client_id` and `venue` were None")

        with pytest.raises(ValueError) as e:
            DataResponse(
                client_id=None,
                venue=None,
                data_type=DataType(QuoteTick),
                data=[],
                correlation_id=UUID4(),
                response_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                start=pd.Timestamp("2023-01-01"),
                end=pd.Timestamp("2023-01-02"),
            )
        assert issubclass(e.type, ValueError)
        assert e.match("Both `client_id` and `venue` were None")

    def test_data_command_str_and_repr(self):
        # Arrange, Act
        command_id = UUID4()

        command = SubscribeData(
            instrument_id=None,
            client_id=None,
            venue=BINANCE,
            data_type=DataType(Data, {"type": "newswire"}),
            params={"filter": "ABC"},
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert (
            str(command)
            == "SubscribeData(client_id=None, venue=BINANCE, data_type=Data{'type': 'newswire'}, params={'filter': 'ABC'})"
        )
        assert repr(command) == (
            f"SubscribeData("
            f"client_id=None, "
            f"venue=BINANCE, "
            f"data_type=Data{{'type': 'newswire'}}, "
            f"id={command_id}, params={{'filter': 'ABC'}})"
        )

    def test_venue_data_command_str_and_repr(self):
        # Arrange, Act
        command_id = UUID4()

        command = SubscribeData(
            instrument_id=None,
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, {"instrument_id": "BTCUSDT"}),
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert (
            str(command)
            == "SubscribeData(client_id=BINANCE, venue=BINANCE, data_type=TradeTick{'instrument_id': 'BTCUSDT'})"
        )
        assert repr(command) == (
            f"SubscribeData("
            f"client_id=BINANCE, "
            f"venue=BINANCE, "
            f"data_type=TradeTick{{'instrument_id': 'BTCUSDT'}}, "
            f"id={command_id})"
        )

    def test_data_request_message_str_and_repr(self):
        # Arrange, Act
        handler = [].append
        request_id = UUID4()

        request = RequestData(
            data_type=DataType(
                Data,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                },
            ),
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=None,
            venue=BINANCE,
            callback=handler,
            request_id=request_id,
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Assert
        assert (
            str(request)
            == "RequestData(data_type=Data{'instrument_id': InstrumentId('SOMETHING.RANDOM')}, instrument_id=SOMETHING.RANDOM, "
            "start=None, end=None, limit=1000, client_id=None, venue=BINANCE)"
        )
        assert repr(request) == (
            f"RequestData(data_type=Data{{'instrument_id': InstrumentId('SOMETHING.RANDOM')}}, "
            f"instrument_id=SOMETHING.RANDOM, start=None, end=None, limit=1000, client_id=None, venue=BINANCE, callback={handler!r}, id={request_id})"
        )

    def test_venue_data_request_message_str_and_repr(self):
        # Arrange, Act
        handler = [].append
        request_id = UUID4()

        request = RequestData(
            data_type=DataType(
                TradeTick,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                },
            ),
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=None,
            venue=BINANCE,
            callback=handler,
            request_id=request_id,
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Assert
        assert (
            str(request)
            == "RequestData(data_type=TradeTick{'instrument_id': InstrumentId('SOMETHING.RANDOM')}, instrument_id=SOMETHING.RANDOM, "
            "start=None, end=None, limit=1000, client_id=None, venue=BINANCE)"
        )
        assert (
            f"RequestData(data_type=TradeTick{{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'limit': 1000}}, "
            f"instrument_id=SOMETHING.RANDOM, start=None, end=None, client_id=None, venue=BINANCE, callback={handler!r}, id={request_id})"
        )

    def test_data_response_message_str_and_repr(self):
        # Arrange, Act
        correlation_id = UUID4()
        response_id = UUID4()
        instrument_id = InstrumentId(Symbol("AUD/USD"), IDEALPRO)

        response = DataResponse(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            data=[],
            correlation_id=correlation_id,
            response_id=response_id,
            ts_init=self.clock.timestamp_ns(),
            start=pd.Timestamp("2023-01-01"),
            end=pd.Timestamp("2023-01-02"),
        )

        # Assert
        assert (
            str(response)
            == "DataResponse(client_id=None, venue=BINANCE, data_type=QuoteTick{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')})"
        )
        assert repr(response) == (
            f"DataResponse("
            f"client_id=None, "
            f"venue=BINANCE, "
            f"data_type=QuoteTick{{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')}}, "
            f"correlation_id={correlation_id}, "
            f"id={response_id})"
        )

    def test_venue_data_response_message_str_and_repr(self):
        # Arrange, Act
        correlation_id = UUID4()
        response_id = UUID4()
        instrument_id = InstrumentId(Symbol("AUD/USD"), IDEALPRO)

        response = DataResponse(
            client_id=ClientId("IB"),
            venue=Venue("IDEALPRO"),
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            data=[],
            correlation_id=correlation_id,
            response_id=response_id,
            ts_init=self.clock.timestamp_ns(),
            start=pd.Timestamp("2023-01-01"),
            end=pd.Timestamp("2023-01-02"),
        )

        # Assert
        assert (
            str(response)
            == "DataResponse(client_id=IB, venue=IDEALPRO, data_type=QuoteTick{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')})"
        )
        assert repr(response) == (
            f"DataResponse("
            f"client_id=IB, "
            f"venue=IDEALPRO, "
            f"data_type=QuoteTick{{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')}}, "
            f"correlation_id={correlation_id}, "
            f"id={response_id})"
        )

    def test_subscribe_order_book_to_request_conversion(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        command_id = UUID4()

        subscribe = SubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDepth10,
            book_type=BookType.L2_MBP,
            client_id=ClientId("TEST"),
            venue=instrument_id.venue,
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
            depth=10,
        )

        callback = [].append
        start = pd.Timestamp("2023-01-01", tz="UTC")
        end = pd.Timestamp("2023-01-02", tz="UTC")

        # Act
        request = subscribe.to_request(start=start, end=end, callback=callback)

        # Assert
        assert isinstance(request, RequestOrderBookDepth)
        assert request.instrument_id == instrument_id
        assert request.start == start
        assert request.end == end
        assert request.depth == 10
        assert request.limit == 0
        assert request.client_id == ClientId("TEST")
        assert request.venue == instrument_id.venue
        assert request.callback == callback
        assert request.id == command_id
        assert "subscription_name" in request.params
        assert request.params["subscription_name"] == "OrderBookDepth10.AUD/USD.SIM"

    def test_subscribe_order_book_to_request_conversion_with_invalid_data_type_raises_error(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        command_id = UUID4()

        subscribe = SubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDelta,  # Invalid data type for order book depth conversion
            book_type=BookType.L2_MBP,
            client_id=ClientId("TEST"),
            venue=instrument_id.venue,
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
            depth=10,
        )

        callback = [].append
        start = pd.Timestamp("2023-01-01", tz="UTC")
        end = pd.Timestamp("2023-01-02", tz="UTC")

        # Act & Assert
        with pytest.raises(ValueError) as e:
            subscribe.to_request(start=start, end=end, callback=callback)

        assert "Cannot convert SubscribeOrderBook with data_type" in str(e.value)
        assert "Only OrderBookDepth10 subscriptions can be converted" in str(e.value)

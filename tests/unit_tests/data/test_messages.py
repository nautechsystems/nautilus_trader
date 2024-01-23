# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.data.messages import Unsubscribe
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
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
            Subscribe(
                client_id=None,
                venue=None,
                data_type=DataType(Data, {"type": "newswire"}),
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
        assert e.type == ValueError
        assert e.match("Both `client_id` and `venue` were None")

        with pytest.raises(ValueError) as e:
            Unsubscribe(
                client_id=None,
                venue=None,
                data_type=DataType(Data, {"type": "newswire"}),
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
        assert e.type == ValueError
        assert e.match("Both `client_id` and `venue` were None")

        with pytest.raises(ValueError) as e:
            handler = []
            DataRequest(
                client_id=None,
                venue=None,
                data_type=DataType(QuoteTick),
                callback=handler.append,
                request_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
        assert e.type == ValueError
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
            )
        assert e.type == ValueError
        assert e.match("Both `client_id` and `venue` were None")

    def test_data_command_str_and_repr(self):
        # Arrange, Act
        command_id = UUID4()

        command = Subscribe(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(Data, {"type": "newswire"}),
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert str(command) == "Subscribe(Data{'type': 'newswire'})"
        assert repr(command) == (
            f"Subscribe("
            f"client_id=None, "
            f"venue=BINANCE, "
            f"data_type=Data{{'type': 'newswire'}}, "
            f"id={command_id})"
        )

    def test_venue_data_command_str_and_repr(self):
        # Arrange, Act
        command_id = UUID4()

        command = Subscribe(
            client_id=ClientId(BINANCE.value),
            venue=BINANCE,
            data_type=DataType(TradeTick, {"instrument_id": "BTCUSDT"}),
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert str(command) == "Subscribe(TradeTick{'instrument_id': 'BTCUSDT'})"
        assert repr(command) == (
            f"Subscribe("
            f"client_id=BINANCE, "
            f"venue=BINANCE, "
            f"data_type=TradeTick{{'instrument_id': 'BTCUSDT'}}, "
            f"id={command_id})"
        )

    def test_data_request_message_str_and_repr(self):
        # Arrange, Act
        handler = [].append
        request_id = UUID4()

        request = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                Data,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler,
            request_id=request_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert (
            str(request)
            == "DataRequest(Data{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'start': None, 'end': None, 'limit': 1000})"
        )
        assert repr(request) == (
            f"DataRequest("
            f"client_id=None, "
            f"venue=BINANCE, "
            f"data_type=Data{{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'start': None, 'end': None, 'limit': 1000}}, "
            f"callback={handler!r}, "
            f"id={request_id})"
        )

    def test_venue_data_request_message_str_and_repr(self):
        # Arrange, Act
        handler = [].append
        request_id = UUID4()

        request = DataRequest(
            client_id=None,
            venue=BINANCE,
            data_type=DataType(
                TradeTick,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "start": None,
                    "end": None,
                    "limit": 1000,
                },
            ),
            callback=handler,
            request_id=request_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert (
            str(request)
            == "DataRequest(TradeTick{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'start': None, 'end': None, 'limit': 1000})"
        )
        assert repr(request) == (
            f"DataRequest("
            f"client_id=None, "
            f"venue=BINANCE, "
            f"data_type=TradeTick{{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'start': None, 'end': None, 'limit': 1000}}, "
            f"callback={handler!r}, "
            f"id={request_id})"
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
        )

        # Assert
        assert (
            str(response)
            == "DataResponse(QuoteTick{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')})"
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
        )

        # Assert
        assert (
            str(response)
            == "DataResponse(QuoteTick{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')})"
        )
        assert repr(response) == (
            f"DataResponse("
            f"client_id=IB, "
            f"venue=IDEALPRO, "
            f"data_type=QuoteTick{{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')}}, "
            f"correlation_id={correlation_id}, "
            f"id={response_id})"
        )

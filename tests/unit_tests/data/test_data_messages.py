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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.tick import QuoteTick
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
        self.uuid_factory = UUIDFactory()

    def test_data_command_str_and_repr(self):
        # Arrange, Act
        command_id = self.uuid_factory.generate()

        command = Subscribe(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(str, {"type": "newswire"}),  # str data type is invalid
            command_id=command_id,
            ts_init=self.clock.timestamp_ns(),
        )

        # Assert
        assert str(command) == "Subscribe(str{'type': 'newswire'})"
        assert repr(command) == (
            f"Subscribe("
            f"client_id=BINANCE, "
            f"data_type=str{{'type': 'newswire'}}, "
            f"id={command_id})"
        )

    def test_data_request_message_str_and_repr(self):
        # Arrange, Act
        handler = [].append
        request_id = self.uuid_factory.generate()

        request = DataRequest(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                str,
                metadata={  # str data type is invalid
                    "instrument_id": InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                    "from_datetime": None,
                    "to_datetime": None,
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
            == "DataRequest(str{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'from_datetime': None, 'to_datetime': None, 'limit': 1000})"
        )
        assert repr(request) == (
            f"DataRequest("
            f"client_id=BINANCE, "
            f"data_type=str{{'instrument_id': InstrumentId('SOMETHING.RANDOM'), 'from_datetime': None, 'to_datetime': None, 'limit': 1000}}, "
            f"callback={repr(handler)}, "
            f"id={request_id})"
        )

    def test_data_response_message_str_and_repr(self):
        # Arrange, Act
        correlation_id = self.uuid_factory.generate()
        response_id = self.uuid_factory.generate()
        instrument_id = InstrumentId(Symbol("AUD/USD"), IDEALPRO)

        response = DataResponse(
            client_id=ClientId(BINANCE.value),
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
            f"client_id=BINANCE, "
            f"data_type=QuoteTick{{'instrument_id': InstrumentId('AUD/USD.IDEALPRO')}}, "
            f"correlation_id={correlation_id}, "
            f"id={response_id})"
        )

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

import pytest

from nautilus_trader.core.type import DataType
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.messages import Subscribe
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.serialization.base import CommandSerializer
from nautilus_trader.serialization.base import EventSerializer
from nautilus_trader.serialization.base import InstrumentSerializer
from nautilus_trader.serialization.base import register_serializable_object
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestObject:
    """
    Represents some generic user object which implements serialization value dicts.
    """

    def __init__(self, value):
        self.value = value

    @staticmethod
    def from_dict(values: dict):
        return TestObject(values["value"])

    @staticmethod
    def to_dict(obj):
        return {"value": obj.value}


class TestSerializationBase:
    def test_register_serializable_object(self):
        # Arrange
        # Act, Assert
        register_serializable_object(TestObject, TestObject.to_dict, TestObject.from_dict)

        # Does not raise exception

    def test_instrument_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        serializer = InstrumentSerializer()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            serializer.serialize(AUDUSD_SIM)

        with pytest.raises(NotImplementedError):
            serializer.deserialize(bytes())

    def test_command_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        command = Subscribe(
            client_id=ClientId("SIM"),
            data_type=DataType(QuoteTick),
            handler=[].append,
            command_id=uuid4(),
            timestamp_ns=0,
        )

        serializer = CommandSerializer()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            serializer.serialize(command)

        with pytest.raises(NotImplementedError):
            serializer.deserialize(bytes())

    def test_event_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        event = TestStubs.event_account_state(TestStubs.account_id())
        serializer = EventSerializer()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            serializer.serialize(event)

        with pytest.raises(NotImplementedError):
            serializer.deserialize(bytes())

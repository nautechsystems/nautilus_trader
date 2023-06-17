# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.serialization.base import register_serializable_object
from nautilus_trader.test_kit.providers import TestInstrumentProvider


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
        # Arrange, Act, Assert
        register_serializable_object(TestObject, TestObject.to_dict, TestObject.from_dict)

        # Does not raise exception

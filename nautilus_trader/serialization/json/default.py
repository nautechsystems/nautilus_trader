# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import decimal
from typing import Callable, Dict


class Default:
    """
    Serialization extensions for orjson.dumps.
    """

    registry: Dict = {}

    @classmethod
    def register_serializer(cls, type_: type, serializer: Callable):
        """Register a new type `type_` for serialization in orjson."""
        assert type_ not in cls.registry
        cls.registry[type_] = serializer

    @classmethod
    def serialize(cls, obj):
        """Serialize for types orjson.dumps can't understand."""
        if type(obj) in cls.registry:
            return cls.registry[type(obj)](obj)
        raise TypeError


Default.register_serializer(type_=decimal.Decimal, serializer=str)

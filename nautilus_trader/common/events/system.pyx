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

import decimal
import json

import msgspec

from libc.stdint cimport uint64_t

from nautilus_trader.common.enums_c cimport ComponentState
from nautilus_trader.common.enums_c cimport component_state_from_str
from nautilus_trader.common.enums_c cimport component_state_to_str
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport TraderId


cdef class ComponentStateChanged(Event):
    """
    Represents an event which includes information on the state of a component.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the event.
    component_id : ComponentId
        The component ID associated with the event.
    component_type : str
        The component type.
    state : ComponentState
        The component state.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the component state event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        ComponentId component_id not None,
        str component_type not None,
        ComponentState state,
        dict config not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        super().__init__(event_id, ts_event, ts_init)

        self.trader_id = trader_id
        self.component_id = component_id
        self.component_type = component_type
        self.state = state
        self.config = config

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"component_id={self.component_id.to_str()}, "
            f"component_type={self.component_type}, "
            f"state={component_state_to_str(self.state)}, "
            f"config={self.config}, "
            f"event_id={self.id.to_str()})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"component_id={self.component_id.to_str()}, "
            f"component_type={self.component_type}, "
            f"state={component_state_to_str(self.state)}, "
            f"config={self.config}, "
            f"event_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef ComponentStateChanged from_dict_c(dict values):
        Condition.not_none(values, "values")
        return ComponentStateChanged(
            trader_id=TraderId(values["trader_id"]),
            component_id=ComponentId(values["component_id"]),
            component_type=values["component_type"],
            state=component_state_from_str(values["state"]),
            config=json.loads(values["config"]),
            event_id=UUID4(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(ComponentStateChanged obj):
        Condition.not_none(obj, "obj")
        cdef:
            bytes config_bytes
        try:
            # TODO(cs): Temporary workaround
            for k, v in obj.config.items():
                if isinstance(v, decimal.Decimal):
                    obj.config[k] = str(v)
            config_bytes = msgspec.json.encode(obj.config)
        except TypeError as e:
            if str(e).startswith("Type is not JSON serializable"):
                type_str = str(e).split(":")[1].strip()
                raise TypeError(
                    f"Cannot serialize config as {e}. "
                    f"You can register a new serializer for `{type_str}` through "
                    f"`Default.register_serializer`.",
                )
            else:
                raise e
        return {
            "type": "ComponentStateChanged",
            "trader_id": obj.trader_id.to_str(),
            "component_id": obj.component_id.to_str(),
            "component_type": obj.component_type,
            "state": component_state_to_str(obj.state),
            "config": config_bytes,
            "event_id": obj.id.to_str(),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> ComponentStateChanged:
        """
        Return a component state changed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        ComponentStateChanged

        """
        return ComponentStateChanged.from_dict_c(values)

    @staticmethod
    def to_dict(ComponentStateChanged obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return ComponentStateChanged.to_dict_c(obj)

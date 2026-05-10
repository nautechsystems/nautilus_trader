# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio

import pytest

from nautilus_trader.common import BusMessage
from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.common import ComponentState
from nautilus_trader.common import ComponentTrigger
from nautilus_trader.common import CustomData
from nautilus_trader.common import Environment
from nautilus_trader.common import GreeksCalculator
from nautilus_trader.common import LogColor
from nautilus_trader.common import LogFormat
from nautilus_trader.common import LogLevel
from nautilus_trader.common import MessageBusListener
from nautilus_trader.common import Signal
from nautilus_trader.common import get_exchange_rate
from nautilus_trader.model import DataType
from nautilus_trader.model import PriceType


@pytest.mark.parametrize(
    ("enum_type", "member", "name"),
    [
        (Environment, Environment.LIVE, "LIVE"),
        (LogColor, LogColor.RED, "RED"),
        (LogLevel, LogLevel.INFO, "INFO"),
    ],
)
def test_common_enums_support_variants_and_from_str(enum_type, member, name):
    assert member in list(enum_type.variants())
    assert enum_type.from_str(name) == member


def test_component_state_and_trigger_surface():
    assert ComponentState.READY != ComponentState.RUNNING
    assert ComponentTrigger.START != ComponentTrigger.STOP
    assert isinstance(hash(ComponentState.READY), int)
    assert isinstance(hash(ComponentTrigger.START), int)


def test_log_format_surface():
    assert LogFormat.BOLD == LogFormat.BOLD
    assert LogFormat.BOLD != LogFormat.ENDC
    assert str(LogFormat.BOLD) == "LogFormat.BOLD"


def test_signal_and_custom_data_fields():
    signal = Signal("sig", "value", 1, 2)
    custom = CustomData(DataType("X"), [1, 2], 3, 4)

    assert signal.name == "sig"
    assert signal.value == "value"
    assert signal.ts_event == 1
    assert signal.ts_init == 2
    assert custom.data_type.type_name == "X"
    assert custom.value == b"\x01\x02"
    assert custom.ts_event == 3
    assert custom.ts_init == 4


def test_get_exchange_rate_direct_and_inverse_pairs():
    direct = get_exchange_rate(
        "USD",
        "EUR",
        PriceType.MID,
        {"USD/EUR": 0.8},
        {"USD/EUR": 0.8},
    )
    inverse = get_exchange_rate(
        "USD",
        "EUR",
        PriceType.MID,
        {"EUR/USD": 1.25},
        {"EUR/USD": 1.25},
    )

    assert direct == pytest.approx(0.8)
    assert inverse == pytest.approx(0.8)


def test_message_bus_listener_stream_requires_running_event_loop():
    listener = MessageBusListener()

    with pytest.raises(RuntimeError, match="running event loop"):
        listener.stream(lambda msg: None)

    listener.close()


def test_message_bus_listener_stream_yields_bus_message():
    async def run_test():
        listener = MessageBusListener()
        received = []

        listener.stream(received.append)
        listener.publish("topic", b"abc")

        for _ in range(10):
            if received:
                break
            await asyncio.sleep(0.01)

        assert listener.is_active() is True
        assert listener.is_closed() is False
        assert len(received) == 1

        message = received[0]

        assert isinstance(message, BusMessage)
        assert message.topic == "topic"
        assert message.payload == b"abc"

        listener.close()
        await asyncio.sleep(0.1)

        assert listener.is_active() is False
        assert listener.is_closed() is True

    asyncio.run(run_test())


def test_greeks_calculator_construction():
    cache = Cache()
    clock = Clock.new_test()
    calc = GreeksCalculator(cache, clock)

    assert isinstance(calc, GreeksCalculator)

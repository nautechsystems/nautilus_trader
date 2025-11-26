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

import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.signal import generate_signal_class
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.identifiers import TraderId


class TestSignalSerialization:
    """
    Tests for signal serialization functionality.
    """

    def test_generate_signal_class_creates_data_subclass(self):
        # Arrange, Act
        SignalClass = generate_signal_class("test_basic", int)

        # Assert
        assert issubclass(SignalClass, Data)
        assert SignalClass.__name__ == "SignalTest_Basic"

    def test_signal_instance_has_required_properties(self):
        # Arrange
        SignalClass = generate_signal_class("price_prop", float)
        ts_event = 1_000_000_000
        ts_init = 1_000_000_001

        # Act
        signal = SignalClass(value=100.5, ts_event=ts_event, ts_init=ts_init)

        # Assert
        assert signal.value == 100.5
        assert signal.ts_event == ts_event
        assert signal.ts_init == ts_init

    def test_signal_has_serialization_methods(self):
        # Arrange
        SignalClass = generate_signal_class("volume_methods", int)

        # Act, Assert
        assert hasattr(SignalClass, "to_dict_c")
        assert hasattr(SignalClass, "from_dict_c")
        assert hasattr(SignalClass, "to_dict")
        assert hasattr(SignalClass, "from_dict")
        assert callable(SignalClass.to_dict_c)
        assert callable(SignalClass.from_dict_c)
        assert callable(SignalClass.to_dict)
        assert callable(SignalClass.from_dict)

    @pytest.mark.parametrize(
        ("value_type", "test_value", "signal_name"),
        [
            (int, 42, "roundtrip_int"),
            (float, 3.14159, "roundtrip_float"),
            (str, "test_signal", "roundtrip_str"),
            (bool, True, "roundtrip_bool"),
            (bytes, b"binary_data", "roundtrip_bytes"),
        ],
    )
    def test_signal_serialization_roundtrip(self, value_type, test_value, signal_name):
        # Arrange
        SignalClass = generate_signal_class(signal_name, value_type)
        ts_event = 1_000_000_000
        ts_init = 1_000_000_001
        original_signal = SignalClass(value=test_value, ts_event=ts_event, ts_init=ts_init)

        # Act - serialize to dict
        signal_dict = SignalClass.to_dict_c(original_signal)

        # Assert - dict contains expected fields
        assert signal_dict["type"] == SignalClass.__name__
        assert signal_dict["value"] == test_value
        assert signal_dict["ts_event"] == ts_event
        assert signal_dict["ts_init"] == ts_init

        # Act - deserialize from dict
        reconstructed_signal = SignalClass.from_dict_c(signal_dict)

        # Assert - reconstructed signal matches original
        assert reconstructed_signal.value == original_signal.value
        assert reconstructed_signal.ts_event == original_signal.ts_event
        assert reconstructed_signal.ts_init == original_signal.ts_init
        assert type(reconstructed_signal).__name__ == type(original_signal).__name__

    def test_signal_to_dict_includes_type_field(self):
        # Arrange
        SignalClass = generate_signal_class("status_type", str)
        signal = SignalClass(value="active", ts_event=1000, ts_init=1001)

        # Act
        signal_dict = SignalClass.to_dict_c(signal)

        # Assert
        assert "type" in signal_dict
        assert signal_dict["type"] == "SignalStatus_Type"

    def test_signal_from_dict_ignores_extra_fields(self):
        # Arrange
        SignalClass = generate_signal_class("extra_fields", int)
        signal_dict = {
            "type": "SignalExtra_Fields",
            "value": 123,
            "ts_event": 1000,
            "ts_init": 1001,
            "extra_field": "should_be_ignored",
        }

        # Act
        signal = SignalClass.from_dict_c(signal_dict)

        # Assert
        assert signal.value == 123
        assert signal.ts_event == 1000
        assert signal.ts_init == 1001
        # Extra field should not cause issues

    def test_signal_public_methods_delegate_to_c_methods(self):
        # Arrange
        SignalClass = generate_signal_class("delegate_test", float)
        signal = SignalClass(value=2.718, ts_event=2000, ts_init=2001)

        # Act
        dict_from_public = signal.to_dict()
        dict_from_c = SignalClass.to_dict_c(signal)

        # Assert
        assert dict_from_public == dict_from_c

        # Act
        signal_from_public = SignalClass.from_dict(dict_from_public)
        signal_from_c = SignalClass.from_dict_c(dict_from_c)

        # Assert
        assert signal_from_public.value == signal_from_c.value
        assert signal_from_public.ts_event == signal_from_c.ts_event
        assert signal_from_public.ts_init == signal_from_c.ts_init

    def test_different_signal_types_have_unique_names(self):
        # Arrange, Act
        IntSignal = generate_signal_class("unique_price", int)
        FloatSignal = generate_signal_class("unique_volume", float)
        StrSignal = generate_signal_class("unique_status", str)

        # Assert
        assert IntSignal.__name__ == "SignalUnique_Price"
        assert FloatSignal.__name__ == "SignalUnique_Volume"
        assert StrSignal.__name__ == "SignalUnique_Status"
        assert IntSignal != FloatSignal
        assert FloatSignal != StrSignal
        assert IntSignal != StrSignal

    def test_signal_serialization_preserves_precision(self):
        # Arrange
        SignalClass = generate_signal_class("precise_test", float)
        precise_value = 1.23456789012345
        signal = SignalClass(value=precise_value, ts_event=3000, ts_init=3001)

        # Act
        signal_dict = SignalClass.to_dict_c(signal)
        reconstructed = SignalClass.from_dict_c(signal_dict)

        # Assert
        assert reconstructed.value == precise_value

    def test_signal_with_zero_timestamps(self):
        # Arrange
        SignalClass = generate_signal_class("zero_ts", int)

        # Act
        signal = SignalClass(value=0, ts_event=0, ts_init=0)
        signal_dict = SignalClass.to_dict_c(signal)
        reconstructed = SignalClass.from_dict_c(signal_dict)

        # Assert
        assert reconstructed.value == 0
        assert reconstructed.ts_event == 0
        assert reconstructed.ts_init == 0

    def test_signal_with_large_timestamps(self):
        # Arrange
        SignalClass = generate_signal_class("large_ts", str)
        large_ts = 9_223_372_036_854_775_807  # Max int64

        # Act
        signal = SignalClass(value="test", ts_event=large_ts, ts_init=large_ts - 1)
        signal_dict = SignalClass.to_dict_c(signal)
        reconstructed = SignalClass.from_dict_c(signal_dict)

        # Assert
        assert reconstructed.ts_event == large_ts
        assert reconstructed.ts_init == large_ts - 1


class TestSignalMessageBusIntegration:
    """
    Tests for signal integration with message bus.
    """

    def setup_method(self):
        """
        Set up test fixtures.
        """
        self.trader_id = TraderId("TESTER-001")
        self.instance_id = UUID4()
        self.clock = TestClock()
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            instance_id=self.instance_id,
        )

    def test_signal_can_be_published_to_message_bus(self):
        # Arrange
        SignalClass = generate_signal_class("msgbus_test", int)
        signal = SignalClass(value=42, ts_event=1000, ts_init=1001)
        received_signals = []

        def handler(msg):
            received_signals.append(msg)

        self.msgbus.subscribe(topic="SignalMsgbus_Test*", handler=handler)

        # Act
        self.msgbus.publish(topic="SignalMsgbus_Test*", msg=signal)

        # Assert
        assert len(received_signals) == 1
        received_signal = received_signals[0]
        assert received_signal.value == signal.value
        assert received_signal.ts_event == signal.ts_event
        assert received_signal.ts_init == signal.ts_init
        assert type(received_signal).__name__ == type(signal).__name__

    def test_multiple_signal_types_can_be_published(self):
        # Arrange
        IntSignal = generate_signal_class("multi_price", int)
        FloatSignal = generate_signal_class("multi_volume", float)
        StrSignal = generate_signal_class("multi_status", str)

        int_signal = IntSignal(value=100, ts_event=1000, ts_init=1001)
        float_signal = FloatSignal(value=1.5, ts_event=2000, ts_init=2001)
        str_signal = StrSignal(value="active", ts_event=3000, ts_init=3001)

        received_signals = []

        def handler(msg):
            received_signals.append(msg)

        # Subscribe to all signal types
        self.msgbus.subscribe(topic="SignalMulti_Price*", handler=handler)
        self.msgbus.subscribe(topic="SignalMulti_Volume*", handler=handler)
        self.msgbus.subscribe(topic="SignalMulti_Status*", handler=handler)

        # Act
        self.msgbus.publish(topic="SignalMulti_Price*", msg=int_signal)
        self.msgbus.publish(topic="SignalMulti_Volume*", msg=float_signal)
        self.msgbus.publish(topic="SignalMulti_Status*", msg=str_signal)

        # Assert
        assert len(received_signals) == 3

        # Verify each signal was received correctly
        received_int = next(s for s in received_signals if isinstance(s.value, int))
        received_float = next(s for s in received_signals if isinstance(s.value, float))
        received_str = next(s for s in received_signals if isinstance(s.value, str))

        assert received_int.value == 100
        assert received_float.value == 1.5
        assert received_str.value == "active"

    def test_signal_wildcard_subscription_works(self):
        # Arrange
        SignalClass1 = generate_signal_class("wildcard_price", int)
        SignalClass2 = generate_signal_class("wildcard_volume", float)

        signal1 = SignalClass1(value=100, ts_event=1000, ts_init=1001)
        signal2 = SignalClass2(value=2.5, ts_event=2000, ts_init=2001)

        received_signals = []

        def handler(msg):
            received_signals.append(msg)

        # Subscribe to all signals with wildcard
        self.msgbus.subscribe(topic="Signal*", handler=handler)

        # Act
        self.msgbus.publish(topic="SignalWildcard_Price*", msg=signal1)
        self.msgbus.publish(topic="SignalWildcard_Volume*", msg=signal2)

        # Assert
        assert len(received_signals) == 2
        assert any(s.value == 100 for s in received_signals)
        assert any(s.value == 2.5 for s in received_signals)

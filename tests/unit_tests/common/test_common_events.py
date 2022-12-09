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
from dataclasses import dataclass

import pytest

from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.events.risk import TradingStateChanged
from nautilus_trader.common.events.system import ComponentStateChanged
from nautilus_trader.config import ActorConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestCommonEvents:
    def test_component_state_changed(self):
        # Arrange
        uuid = UUID4()
        event = ComponentStateChanged(
            trader_id=TestIdStubs.trader_id(),
            component_id=ComponentId("MyActor-001"),
            component_type="MyActor",
            state=ComponentState.RUNNING,
            config={"do_something": True},
            event_id=uuid,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert ComponentStateChanged.from_dict(ComponentStateChanged.to_dict(event)) == event
        assert (
            str(event)
            == f"ComponentStateChanged(trader_id=TESTER-000, component_id=MyActor-001, component_type=MyActor, state=RUNNING, config={{'do_something': True}}, event_id={uuid})"  # noqa
        )
        assert (
            repr(event)
            == f"ComponentStateChanged(trader_id=TESTER-000, component_id=MyActor-001, component_type=MyActor, state=RUNNING, config={{'do_something': True}}, event_id={uuid}, ts_init=0)"  # noqa
        )

    def test_serializing_component_state_changed_with_unserializable_config_raises_helpful_exception(
        self,
    ):
        # Arrange

        class MyType(ActorConfig):
            values: list[int]

        config = {"key": MyType(values=[1, 2, 3])}
        event = ComponentStateChanged(
            trader_id=TestIdStubs.trader_id(),
            component_id=ComponentId("MyActor-001"),
            component_type="MyActor",
            state=ComponentState.RUNNING,
            config=config,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        with pytest.raises(TypeError) as e:
            TradingStateChanged.to_dict(event)

            # Assert
            assert e.value == TypeError(
                "Cannot serialize config as Type is not JSON serializable: MyType. You can register a new serializer for `MyType` through `Default.register_serializer`.",  # noqa
            )  # noqa

    def test_trading_state_changed(self):
        # Arrange
        uuid = UUID4()
        event = TradingStateChanged(
            trader_id=TestIdStubs.trader_id(),
            state=TradingState.HALTED,
            config={"max_order_submit_rate": "100/00:00:01"},
            event_id=uuid,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert TradingStateChanged.from_dict(TradingStateChanged.to_dict(event)) == event
        assert (
            str(event)
            == f"TradingStateChanged(trader_id=TESTER-000, state=HALTED, config={{'max_order_submit_rate': '100/00:00:01'}}, event_id={uuid})"  # noqa
        )
        assert (
            repr(event)
            == f"TradingStateChanged(trader_id=TESTER-000, state=HALTED, config={{'max_order_submit_rate': '100/00:00:01'}}, event_id={uuid}, ts_init=0)"  # noqa
        )

    @pytest.mark.skip(reason="msgspec no longer raises an exception")
    def test_serializing_trading_state_changed_with_unserializable_config_raises_helpful_exception(
        self,
    ):
        # Arrange

        @dataclass
        class MyType:
            values: list[int]

        config = {"key": MyType(values=[1, 2, 3])}
        event = TradingStateChanged(
            trader_id=TestIdStubs.trader_id(),
            state=TradingState.HALTED,
            config=config,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        with pytest.raises(TypeError) as e:
            TradingStateChanged.to_dict(event)

        # Assert
        expected = "Serialization failed: `Encoding objects of type MyType is unsupported`. You can register a new serializer for `MyType` through `nautilus_trader.config.backtest.register_json_encoding`."  # noqa
        msg = e.value.args[0]
        assert msg == expected
